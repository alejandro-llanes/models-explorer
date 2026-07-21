//! [`BenchCache`] — an atomic per-provider JSON cache for [`ProviderData`].
//!
//! Mirrors `modelx-cache`, but stores one file per benchmark provider under a
//! `benchmarks/` subdirectory of the platform cache dir.

use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use directories::ProjectDirs;

use crate::error::BenchError;
use crate::provider::ProviderData;

/// Atomic on-disk cache of per-provider benchmark data.
pub struct BenchCache {
    base_dir: PathBuf,
}

impl BenchCache {
    /// Discover the platform cache directory (`.../modelx/.../benchmarks`).
    pub fn discover() -> Result<BenchCache, BenchError> {
        let proj = ProjectDirs::from("dev", "modelx", "modelx").ok_or(BenchError::NoCacheDir)?;
        Ok(BenchCache {
            base_dir: proj.cache_dir().join("benchmarks"),
        })
    }

    /// Build a cache rooted at an explicit directory (for tests and overrides).
    pub fn with_dir(dir: PathBuf) -> BenchCache {
        BenchCache { base_dir: dir }
    }

    /// Path where `provider_id`'s data is stored.
    pub fn path_for(&self, provider_id: &str) -> PathBuf {
        let safe = sanitize_id(provider_id);
        self.base_dir.join(format!("{safe}.json"))
    }

    /// Load a provider's data. Returns `Ok(None)` when the file is missing.
    pub fn load(&self, provider_id: &str) -> Result<Option<ProviderData>, BenchError> {
        let path = self.path_for(provider_id);
        match fs::read(&path) {
            Ok(bytes) => {
                let data =
                    serde_json::from_slice(&bytes).map_err(|e| BenchError::Parse(e.to_string()))?;
                Ok(Some(data))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(BenchError::Io(e.to_string())),
        }
    }

    /// Persist a provider's data atomically (tmp write + rename).
    pub fn store(&self, data: &ProviderData) -> Result<(), BenchError> {
        let path = self.path_for(&data.provider_id);
        let parent = path.parent().expect("path_for always has a parent");
        fs::create_dir_all(parent).map_err(|e| BenchError::Io(e.to_string()))?;

        let tmp_path = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(data).map_err(|e| BenchError::Parse(e.to_string()))?;
        fs::write(&tmp_path, &bytes).map_err(|e| BenchError::Io(e.to_string()))?;
        fs::rename(&tmp_path, &path).map_err(|e| BenchError::Io(e.to_string()))?;
        Ok(())
    }

    /// Seconds since the cached file's last modification, or `None` if missing.
    pub fn age_seconds(&self, provider_id: &str) -> Option<i64> {
        let path = self.path_for(provider_id);
        let meta = fs::metadata(&path).ok()?;
        let mtime = meta.modified().ok()?;
        let elapsed = SystemTime::now().duration_since(mtime).ok()?;
        Some(elapsed.as_secs() as i64)
    }

    /// `true` if the entry is missing or older than `ttl_seconds`.
    pub fn is_stale(&self, provider_id: &str, ttl_seconds: i64) -> bool {
        match self.age_seconds(provider_id) {
            None => true,
            Some(age) => age > ttl_seconds,
        }
    }
}

/// Replace filesystem-unsafe characters in a provider id with `_`.
fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            c => c,
        })
        .collect()
}
