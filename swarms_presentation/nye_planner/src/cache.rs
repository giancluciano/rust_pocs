//! Tiny on-disk response cache so reruns and overlapping searches don't hit
//! the website again.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone)]
pub struct DiskCache {
    dir: PathBuf,
    ttl: Duration,
}

impl DiskCache {
    pub fn new(dir: impl Into<PathBuf>, ttl: Duration) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir, ttl })
    }

    fn path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{:016x}.cache", fnv1a(key)))
    }

    pub fn get(&self, key: &str) -> Option<String> {
        let path = self.path(key);
        let age = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())?;
        if age > self.ttl {
            return None;
        }
        std::fs::read_to_string(path).ok()
    }

    /// Best effort: a failed write only costs a future re-download.
    pub fn put(&self, key: &str, body: &str) {
        let path = self.path(key);
        if let Err(e) = write_atomic(&path, body) {
            tracing::warn!(path = %path.display(), error = %e, "cache write failed");
        }
    }
}

fn write_atomic(path: &Path, body: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(tmp, path)
}

/// FNV-1a: stable across Rust versions (unlike `DefaultHasher`), so cache
/// file names survive toolchain upgrades.
fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path(), Duration::from_secs(60)).unwrap();
        assert_eq!(cache.get("GET https://a"), None);
        cache.put("GET https://a", "body");
        assert_eq!(cache.get("GET https://a").as_deref(), Some("body"));
        assert_eq!(cache.get("GET https://b"), None);
    }

    #[test]
    fn expired_entries_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path(), Duration::ZERO).unwrap();
        cache.put("k", "v");
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(cache.get("k"), None);
    }

    #[test]
    fn fnv_is_stable() {
        assert_eq!(fnv1a(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a("a"), 0xaf63_dc4c_8601_ec8c);
    }
}
