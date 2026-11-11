//! Text files for the editor tab: decoding / encoding while keeping the original
//! encoding, byte order mark and line endings, so that saving a file only changes
//! what was edited.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// Largest file the editor opens.
pub const MAX_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16le,
    /// any other 8-bit text (ISO-8859-1 / Windows-1252 subset)
    Latin1,
}

/// A decoded text file. `text` always uses "\n"; `crlf` tells how to write it back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextFile {
    pub text: String,
    pub encoding: Encoding,
    pub crlf: bool,
}

pub fn decode(bytes: &[u8]) -> AppResult<TextFile> {
    let (text, encoding) = if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        (utf8(rest)?, Encoding::Utf8Bom)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        (String::from_utf16_lossy(&units), Encoding::Utf16le)
    } else {
        if bytes.iter().take(64 * 1024).any(|&b| b == 0) {
            return Err(AppError::Unsupported("fichier binaire : il ne peut pas être ouvert dans l'éditeur".into()));
        }
        match std::str::from_utf8(bytes) {
            Ok(s) => (s.to_string(), Encoding::Utf8),
            Err(_) => (bytes.iter().map(|&b| b as char).collect(), Encoding::Latin1),
        }
    };
    let crlf = text.contains("\r\n");
    let text = if crlf { text.replace("\r\n", "\n") } else { text };
    Ok(TextFile { text, encoding, crlf })
}

fn utf8(b: &[u8]) -> AppResult<String> {
    Ok(String::from_utf8_lossy(b).into_owned())
}

pub fn encode(f: &TextFile) -> AppResult<Vec<u8>> {
    let text = if f.crlf { f.text.replace("\r\n", "\n").replace('\n', "\r\n") } else { f.text.clone() };
    Ok(match f.encoding {
        Encoding::Utf8 => text.into_bytes(),
        Encoding::Utf8Bom => [&[0xEF, 0xBB, 0xBF][..], text.as_bytes()].concat(),
        Encoding::Utf16le => {
            let mut out = vec![0xFF, 0xFE];
            for u in text.encode_utf16() {
                out.extend_from_slice(&u.to_le_bytes());
            }
            out
        }
        Encoding::Latin1 => {
            let mut out = Vec::with_capacity(text.len());
            for c in text.chars() {
                let n = c as u32;
                if n > 0xFF {
                    return Err(AppError::Unsupported(format!(
                        "le caractère « {c} » n'existe pas dans l'encodage du fichier (Latin-1)"
                    )));
                }
                out.push(n as u8);
            }
            out
        }
    })
}

/// Local file: written next to the target then renamed, so a failed write keeps the old file.
pub fn write_local(path: &std::path::Path, bytes: &[u8]) -> AppResult<()> {
    let tmp = path.with_extension(format!(
        "{}orbiterm-tmp",
        path.extension().map(|e| format!("{}.", e.to_string_lossy())).unwrap_or_default()
    ));
    std::fs::write(&tmp, bytes)?;
    if let Ok(m) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, m.permissions()); // e.g. keeps the executable bit
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(bytes: &[u8]) -> TextFile {
        let f = decode(bytes).unwrap();
        assert_eq!(encode(&f).unwrap(), bytes, "{f:?}");
        f
    }

    #[test]
    fn keeps_encoding_and_line_ends() {
        let f = roundtrip(b"a\nb\n");
        assert!(!f.crlf && f.encoding == Encoding::Utf8);
        let f = roundtrip(b"a\r\nb\r\n");
        assert!(f.crlf && f.text == "a\nb\n");
        let f = roundtrip("\u{feff}été\r\n".as_bytes());
        assert_eq!((f.encoding, f.text.as_str()), (Encoding::Utf8Bom, "été\n"));
        let f = roundtrip(&[0xFF, 0xFE, b'h', 0, b'i', 0, b'\r', 0, b'\n', 0]);
        assert_eq!((f.encoding, f.text.as_str()), (Encoding::Utf16le, "hi\n"));
        let f = roundtrip(&[b'c', b'a', b'f', 0xE9]);
        assert_eq!((f.encoding, f.text.as_str()), (Encoding::Latin1, "café"));
    }

    #[test]
    fn edits_are_written_with_the_original_format() {
        let mut f = decode(b"x\r\ny\r\n").unwrap();
        f.text.push_str("z\n");
        assert_eq!(encode(&f).unwrap(), b"x\r\ny\r\nz\r\n");
        let mut l = decode(&[0xE9]).unwrap();
        l.text.push('€');
        assert!(encode(&l).is_err());
    }

    #[test]
    fn rejects_binary() {
        assert!(decode(b"\x7fELF\0\0\x01").is_err());
    }

    #[test]
    fn local_write_replaces() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("f.conf");
        std::fs::write(&p, "old").unwrap();
        write_local(&p, b"new").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
    }
}
