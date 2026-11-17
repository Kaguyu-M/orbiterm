//! Persistent bitmap cache of the RDP graphics pipeline ([MS-RDPEGFX] 3.3.1.5), like the
//! `bcache*.bmc` files of the Windows client: the tiles the server stored in the client's
//! bitmap cache are kept when the session ends and offered at the next connection, so the
//! server pastes them instead of sending them again (faster first picture and reconnection).
//!
//! One file per server and account in the cache directory (never in the configuration, which
//! can be exported): `ORBC1`, entry count (u32 LE), then for each entry the key (u64 LE),
//! width and height (u16 LE) and the RGBA pixels.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use ironrdp_egfx::cache::{CachedBitmap, PersistentCacheEntry, PersistentCacheError, PersistentCacheStore};
use ironrdp_egfx::pdu::CacheEntryMetadata;
use sha2::{Digest, Sha256};

const MAGIC: &[u8; 5] = b"ORBC1";
/// Larger files are not read (a damaged or foreign file).
const MAX_FILE: u64 = 128 << 20;

/// Cache file of a server and account.
pub fn cache_file(dir: &Path, host: &str, port: u16, user: &str) -> PathBuf {
    let id = Sha256::digest(format!("{}:{port}:{}", host.to_lowercase(), user.to_lowercase()));
    let name: String = id[..12].iter().map(|b| format!("{b:02x}")).collect();
    dir.join(format!("{name}.bmc"))
}

pub struct FileCache {
    path: PathBuf,
    /// entries read from the file, in their stored order (most recently used first)
    order: Vec<u64>,
    loaded: HashMap<u64, PersistentCacheEntry>,
}

impl FileCache {
    pub fn new(path: PathBuf) -> Self {
        FileCache { path, order: Vec::new(), loaded: HashMap::new() }
    }

    fn read(path: &Path) -> std::io::Result<Vec<PersistentCacheEntry>> {
        let f = std::fs::File::open(path)?;
        if f.metadata()?.len() > MAX_FILE {
            return Err(std::io::Error::other("fichier de cache trop gros"));
        }
        let mut data = Vec::new();
        f.take(MAX_FILE).read_to_end(&mut data)?;
        parse(&data).ok_or_else(|| std::io::Error::other("fichier de cache illisible"))
    }
}

fn parse(data: &[u8]) -> Option<Vec<PersistentCacheEntry>> {
    let rest = data.strip_prefix(MAGIC.as_slice())?;
    let (count, mut rest) = rest.split_first_chunk::<4>()?;
    let mut out = Vec::new();
    for _ in 0..u32::from_le_bytes(*count) {
        let (key, r) = rest.split_first_chunk::<8>()?;
        let (w, r) = r.split_first_chunk::<2>()?;
        let (h, r) = r.split_first_chunk::<2>()?;
        let (width, height) = (u16::from_le_bytes(*w), u16::from_le_bytes(*h));
        let len = usize::from(width) * usize::from(height) * 4;
        if len == 0 || len > ironrdp_egfx::cache::MAX_CACHE_IMPORT_ENTRY_BYTES || r.len() < len {
            return None;
        }
        let (pixels, r) = r.split_at(len);
        out.push(PersistentCacheEntry { cache_key: u64::from_le_bytes(*key), width, height, data: pixels.to_vec() });
        rest = r;
    }
    Some(out)
}

fn encode(entries: &[CachedBitmap<'_>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(9 + entries.iter().map(|e| 12 + e.data.len()).sum::<usize>());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for e in entries {
        out.extend_from_slice(&e.cache_key.to_le_bytes());
        out.extend_from_slice(&e.width.to_le_bytes());
        out.extend_from_slice(&e.height.to_le_bytes());
        out.extend_from_slice(e.data);
    }
    out
}

impl PersistentCacheStore for FileCache {
    fn entries(&mut self) -> Result<Vec<CacheEntryMetadata>, PersistentCacheError> {
        let entries = match Self::read(&self.path) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(Box::new(e)),
        };
        self.order = entries.iter().map(|e| e.cache_key).collect();
        let meta = entries
            .iter()
            .map(|e| CacheEntryMetadata { cache_key: e.cache_key, bitmap_len: e.data.len() as u32 })
            .collect();
        self.loaded = entries.into_iter().map(|e| (e.cache_key, e)).collect();
        Ok(meta)
    }

    fn load(&mut self, cache_key: u64) -> Result<Option<PersistentCacheEntry>, PersistentCacheError> {
        Ok(self.loaded.remove(&cache_key))
    }

    fn import_finished(&mut self) {
        self.loaded.clear();
        self.order.clear();
    }

    fn save(&mut self, entries: &[CachedBitmap<'_>]) -> Result<(), PersistentCacheError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // written next to the file then renamed: a session cut while saving leaves the old cache
        let tmp = self.path.with_extension("tmp");
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&encode(entries))?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_offers_the_entries_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = cache_file(dir.path(), "Host", 3389, "Raph");
        assert_eq!(path, cache_file(dir.path(), "host", 3389, "raph"));
        let a = vec![7u8; 64 * 64 * 4];
        let b = vec![9u8; 16 * 8 * 4];
        let mut store = FileCache::new(path.clone());
        assert!(store.entries().unwrap().is_empty());
        store
            .save(&[
                CachedBitmap { cache_key: 1, width: 64, height: 64, data: &a },
                CachedBitmap { cache_key: 2, width: 16, height: 8, data: &b },
            ])
            .unwrap();
        let mut again = FileCache::new(path.clone());
        let meta = again.entries().unwrap();
        assert_eq!(meta.iter().map(|m| (m.cache_key, m.bitmap_len)).collect::<Vec<_>>(), vec![(1, 16384), (2, 512)]);
        let e = again.load(2).unwrap().unwrap();
        assert_eq!((e.width, e.height, e.data), (16, 8, b));
        assert!(again.load(3).unwrap().is_none());
        // a damaged file is reported, not trusted
        std::fs::write(&path, b"ORBC1\x05\x00\x00\x00garbage").unwrap();
        assert!(FileCache::new(path).entries().is_err());
    }
}
