//! Minimal static HTTP/1.1 file server (GET/HEAD, directory listing, byte ranges).

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::{JoinHandle, JoinSet};

use super::Ctx;
use crate::sftp_server::{confine, virt};

pub async fn start(addr: SocketAddr, ctx: Ctx, stop: oneshot::Receiver<()>) -> std::io::Result<(JoinHandle<()>, u16)> {
    let listener = TcpListener::bind(addr).await?;
    let port = listener.local_addr()?.port();
    let ctx = Arc::new(ctx);
    let task = tokio::spawn(async move {
        let mut conns = JoinSet::new();
        let accept = async {
            loop {
                let Ok((s, peer)) = listener.accept().await else { break };
                let ctx = ctx.clone();
                conns.spawn(async move {
                    let _g = ctx.log.client();
                    let _ = handle(s, peer, &ctx).await;
                });
                while conns.try_join_next().is_some() {}
            }
        };
        tokio::select! {
            _ = stop => {}
            _ = accept => {}
        }
    });
    Ok((task, port))
}

async fn handle(mut s: TcpStream, peer: SocketAddr, ctx: &Ctx) -> std::io::Result<()> {
    // keep-alive: serve requests until the client closes or stays idle; bytes after a
    // request head (pipelined requests) stay in `head` for the next round
    let mut head = Vec::with_capacity(1024);
    let mut buf = [0u8; 2048];
    loop {
        let end = loop {
            if let Some(p) = head.windows(4).position(|w| w == b"\r\n\r\n") {
                break p;
            }
            if head.len() > 16 * 1024 {
                return respond(&mut s, 431, "text/plain", b"Request Header Fields Too Large", false, true).await;
            }
            let n = match tokio::time::timeout(Duration::from_secs(30), s.read(&mut buf)).await {
                Ok(Ok(n)) => n,
                _ => return Ok(()),
            };
            if n == 0 {
                return Ok(());
            }
            head.extend_from_slice(&buf[..n]);
        };
        let text = String::from_utf8_lossy(&head[..end]).into_owned();
        head.drain(..end + 4);
        let mut lines = text.split("\r\n");
        let mut req = lines.next().unwrap_or("").split(' ');
        let (method, target, version) = (req.next().unwrap_or(""), req.next().unwrap_or("/"), req.next().unwrap_or(""));
        let headers: Vec<(String, String)> = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
            .collect();
        let header = |k: &str| headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        let close = version != "HTTP/1.1" || header("connection").is_some_and(|c| c.eq_ignore_ascii_case("close"));
        let status = serve(&mut s, method, target, header("range"), ctx, close).await?;
        ctx.log.log(format!("{} {method} {} → {status}", peer.ip(), percent_decode(target.split('?').next().unwrap_or("/"))));
        if close {
            return Ok(());
        }
    }
}

async fn serve(
    s: &mut TcpStream,
    method: &str,
    target: &str,
    range: Option<&str>,
    ctx: &Ctx,
    close: bool,
) -> std::io::Result<u16> {
    let head = method == "HEAD";
    if method != "GET" && !head {
        respond(s, 405, "text/plain; charset=utf-8", "Méthode non autorisée".as_bytes(), false, close).await?;
        return Ok(405);
    }
    let path = percent_decode(target.split(['?', '#']).next().unwrap_or("/"));
    let real = confine(&ctx.root, &path);
    let md = match tokio::fs::metadata(&real).await {
        Ok(m) => m,
        Err(_) => {
            respond(s, 404, "text/html; charset=utf-8", page("404 — introuvable", &escape(&path)).as_bytes(), head, close).await?;
            return Ok(404);
        }
    };
    if md.is_dir() {
        if !path.ends_with('/') {
            let loc = format!("{}/", virt(&path));
            let msg = format!(
                "HTTP/1.1 301 Moved Permanently\r\nLocation: {}\r\nContent-Length: 0\r\n{}\r\n",
                percent_encode(&loc),
                if close { "Connection: close\r\n" } else { "" }
            );
            s.write_all(msg.as_bytes()).await?;
            return Ok(301);
        }
        let index = real.join("index.html");
        if index.is_file() {
            return send_file(s, &index, range, head, close).await;
        }
        if !ctx.cfg.listing {
            respond(s, 403, "text/html; charset=utf-8", page("403 — accès refusé", "Liste des dossiers désactivée").as_bytes(), head, close).await?;
            return Ok(403);
        }
        let body = listing(&real, &virt(&path)).await;
        respond(s, 200, "text/html; charset=utf-8", body.as_bytes(), head, close).await?;
        return Ok(200);
    }
    send_file(s, &real, range, head, close).await
}

async fn send_file(s: &mut TcpStream, path: &Path, range: Option<&str>, head: bool, close: bool) -> std::io::Result<u16> {
    let mut f = tokio::fs::File::open(path).await?;
    let len = f.metadata().await?.len();
    let mime = mime(path);
    let conn = if close { "Connection: close\r\n" } else { "" };
    // single "bytes=a-b" range (resumable downloads, media seeking)
    let span = range.and_then(|r| parse_range(r, len));
    let (status, start, count, extra) = match span {
        Some((a, b)) => (206, a, b - a + 1, format!("Content-Range: bytes {a}-{b}/{len}\r\n")),
        None if range.is_some() && len > 0 => {
            let msg = format!("HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{len}\r\nContent-Length: 0\r\n{conn}\r\n");
            s.write_all(msg.as_bytes()).await?;
            return Ok(416);
        }
        None => (200, 0, len, String::new()),
    };
    let reason = if status == 206 { "Partial Content" } else { "OK" };
    let hdr = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {mime}\r\nContent-Length: {count}\r\nAccept-Ranges: bytes\r\n{extra}Server: Orbiterm\r\n{conn}\r\n"
    );
    s.write_all(hdr.as_bytes()).await?;
    if !head {
        f.seek(std::io::SeekFrom::Start(start)).await?;
        tokio::io::copy(&mut f.take(count), s).await?;
    }
    Ok(status)
}

fn parse_range(r: &str, len: u64) -> Option<(u64, u64)> {
    let spec = r.strip_prefix("bytes=")?;
    if spec.contains(',') || len == 0 {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (a, b) = match (a.trim(), b.trim()) {
        ("", n) => {
            let n: u64 = n.parse().ok()?;
            (len.saturating_sub(n), len - 1)
        }
        (a, "") => (a.parse().ok()?, len - 1),
        (a, b) => (a.parse().ok()?, b.parse::<u64>().ok()?.min(len - 1)),
    };
    (a <= b && a < len).then_some((a, b))
}

async fn respond(s: &mut TcpStream, code: u16, ctype: &str, body: &[u8], head: bool, close: bool) -> std::io::Result<()> {
    let reason = match code {
        200 => "OK",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let hdr = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nServer: Orbiterm\r\n{}\r\n",
        body.len(),
        if close { "Connection: close\r\n" } else { "" }
    );
    s.write_all(hdr.as_bytes()).await?;
    if !head {
        s.write_all(body).await?;
    }
    Ok(())
}

async fn listing(dir: &Path, vpath: &str) -> String {
    let mut entries: Vec<(String, bool, u64)> = vec![];
    if let Ok(mut rd) = tokio::fs::read_dir(dir).await {
        while let Ok(Some(e)) = rd.next_entry().await {
            let md = e.metadata().await.ok();
            entries.push((
                e.file_name().to_string_lossy().into_owned(),
                md.as_ref().is_some_and(|m| m.is_dir()),
                md.map(|m| m.len()).unwrap_or(0),
            ));
        }
    }
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    let base = if vpath.ends_with('/') { vpath.to_string() } else { format!("{vpath}/") };
    let mut rows = String::new();
    if base != "/" {
        rows.push_str("<tr><td><a href=\"../\">📁 ..</a></td><td></td></tr>");
    }
    for (name, is_dir, size) in entries {
        let href = percent_encode(&format!("{base}{name}{}", if is_dir { "/" } else { "" }));
        let icon = if is_dir { "📁" } else { "📄" };
        let size = if is_dir { String::new() } else { human(size) };
        rows.push_str(&format!("<tr><td><a href=\"{href}\">{icon} {}</a></td><td class=s>{size}</td></tr>", escape(&name)));
    }
    page(
        &format!("Index de {}", escape(&base)),
        &format!("<table>{rows}</table><p class=f>Serveur HTTP Orbiterm</p>"),
    )
}

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=fr><head><meta charset=utf-8><meta name=viewport content=\"width=device-width\"><title>{title}</title>\
<style>body{{font:14px system-ui,sans-serif;margin:24px;color:#222}}h1{{font-size:18px}}table{{border-collapse:collapse;min-width:50%}}\
td{{padding:3px 12px 3px 0}}a{{text-decoration:none;color:#1a5fb4}}a:hover{{text-decoration:underline}}.s{{color:#777;text-align:right}}.f{{color:#999;font-size:12px;margin-top:20px}}</style>\
</head><body><h1>{title}</h1>{body}</body></html>"
    )
}

fn human(n: u64) -> String {
    const U: [&str; 5] = ["o", "Ko", "Mo", "Go", "To"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{n} o") } else { format!("{v:.1} {}", U[i]) }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn mime(p: &Path) -> &'static str {
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "txt" | "log" | "md" | "ini" | "conf" | "cfg" | "sh" | "py" | "rs" | "toml" | "yaml" | "yml" | "csv" => {
            "text/plain; charset=utf-8"
        }
        "xml" => "application/xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_and_encoding() {
        assert_eq!(parse_range("bytes=0-9", 100), Some((0, 9)));
        assert_eq!(parse_range("bytes=90-", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=50-500", 100), Some((50, 99)));
        assert_eq!(parse_range("bytes=200-300", 100), None);
        assert_eq!(percent_decode("/a%20b/%C3%A9t%C3%A9"), "/a b/été");
        assert_eq!(percent_decode("/100%"), "/100%");
        assert_eq!(percent_encode("/a b/é"), "/a%20b/%C3%A9");
    }
}
