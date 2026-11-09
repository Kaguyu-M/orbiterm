//! Browser sessions: a child webview placed over the tab area of the main window.
//!
//! Remote pages get no access to Orbiterm's commands (Tauri only exposes the IPC
//! to the app's own origin unless a capability lists remote URLs).

use serde::Serialize;
use tauri::webview::{PageLoadEvent, WebviewBuilder};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, WebviewUrl};

use crate::error::{AppError, AppResult};

#[derive(Clone, Serialize)]
struct BrowserNav {
    label: String,
    url: String,
    loading: bool,
}

/// "example.com" -> "https://example.com"; keeps explicit schemes (http, https, file…).
pub fn normalize_url(input: &str) -> AppResult<Url> {
    let s = input.trim();
    if s.is_empty() {
        return Err(AppError::Config("adresse vide".into()));
    }
    let with_scheme = if s.contains("://") || s.starts_with("about:") {
        s.to_string()
    } else if s.starts_with("localhost") || s.starts_with("127.") || s.starts_with('[') {
        format!("http://{s}")
    } else {
        format!("https://{s}")
    };
    Url::parse(&with_scheme).map_err(|e| AppError::Config(format!("adresse invalide « {s} » : {e}")))
}

fn webview(app: &AppHandle, label: &str) -> AppResult<tauri::Webview> {
    app.get_webview(label).ok_or_else(|| AppError::NotFound(label.into()))
}

fn terr(e: tauri::Error) -> AppError {
    AppError::Conn(e.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn browser_open(app: AppHandle, label: String, url: String, x: f64, y: f64, w: f64, h: f64) -> AppResult<String> {
    let url = normalize_url(&url)?;
    if let Some(existing) = app.get_webview(&label) {
        existing.navigate(url.clone()).map_err(terr)?;
        return Ok(url.to_string());
    }
    let window = app.get_window("main").ok_or_else(|| AppError::NotFound("main".into()))?;
    let emit_app = app.clone();
    let lab = label.clone();
    let builder = WebviewBuilder::new(&label, WebviewUrl::External(url.clone())).on_page_load(move |_wv, payload| {
        let _ = emit_app.emit(
            "browser-nav",
            BrowserNav { label: lab.clone(), url: payload.url().to_string(), loading: payload.event() == PageLoadEvent::Started },
        );
    });
    window
        .add_child(builder, LogicalPosition::new(x, y), LogicalSize::new(w.max(1.0), h.max(1.0)))
        .map_err(terr)?;
    Ok(url.to_string())
}

#[tauri::command]
pub async fn browser_bounds(app: AppHandle, label: String, x: f64, y: f64, w: f64, h: f64, visible: bool) -> AppResult<()> {
    let wv = webview(&app, &label)?;
    if visible && w > 1.0 && h > 1.0 {
        wv.set_position(LogicalPosition::new(x, y)).map_err(terr)?;
        wv.set_size(LogicalSize::new(w, h)).map_err(terr)?;
        wv.show().map_err(terr)?;
    } else {
        wv.hide().map_err(terr)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn browser_navigate(app: AppHandle, label: String, url: String) -> AppResult<String> {
    let url = normalize_url(&url)?;
    webview(&app, &label)?.navigate(url.clone()).map_err(terr)?;
    Ok(url.to_string())
}

/// "back" | "forward" | "reload" | "stop"
#[tauri::command]
pub async fn browser_action(app: AppHandle, label: String, action: String) -> AppResult<()> {
    let js = match action.as_str() {
        "back" => "history.back()",
        "forward" => "history.forward()",
        "reload" => "location.reload()",
        "stop" => "window.stop()",
        other => return Err(AppError::Config(format!("action inconnue : {other}"))),
    };
    webview(&app, &label)?.eval(js).map_err(terr)
}

#[tauri::command]
pub async fn browser_close(app: AppHandle, label: String) -> AppResult<()> {
    if let Some(wv) = app.get_webview(&label) {
        wv.close().map_err(terr)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(normalize_url("example.com").unwrap().as_str(), "https://example.com/");
        assert_eq!(normalize_url("localhost:8080/x").unwrap().as_str(), "http://localhost:8080/x");
        assert_eq!(normalize_url("127.0.0.1:3000").unwrap().as_str(), "http://127.0.0.1:3000/");
        assert_eq!(normalize_url("http://intranet/a?b=1").unwrap().as_str(), "http://intranet/a?b=1");
        assert!(normalize_url("  ").is_err());
    }
}
