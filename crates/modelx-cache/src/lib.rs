//! `modelx-cache` — atomic on-disk cache of catalogs in the platform cache dir.
//!
//! See `docs/architecture.md`.

use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use directories::ProjectDirs;
use modelx_core::Catalog;
use thiserror::Error;

/// Errors that can occur during cache operations.
#[derive(Debug, Error)]
pub enum CacheError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("could not determine platform cache directory")]
    NoCacheDir,
}

/// Atomic on-disk catalog cache.
pub struct Cache {
    base_dir: PathBuf,
}

impl Cache {
    /// Discover the platform cache directory via `directories`.
    ///
    /// Uses `ProjectDirs::from("dev", "modelx", "modelx")` and returns
    /// `CacheError::NoCacheDir` if the platform does not provide one.
    pub fn discover() -> Result<Cache, CacheError> {
        let proj = ProjectDirs::from("dev", "modelx", "modelx").ok_or(CacheError::NoCacheDir)?;
        Ok(Cache {
            base_dir: proj.cache_dir().to_path_buf(),
        })
    }

    /// Build a cache rooted at an explicit directory (for tests and overrides).
    pub fn with_dir(dir: PathBuf) -> Cache {
        Cache { base_dir: dir }
    }

    /// Return the path where `source_id`'s catalog is stored.
    ///
    /// The id is sanitised so characters that are unsafe in file names
    /// (path separators and other specials) are replaced with `_`.
    pub fn path_for(&self, source_id: &str) -> PathBuf {
        let safe_id = sanitize_id(source_id);
        self.base_dir
            .join("sources")
            .join(format!("{safe_id}.json"))
    }

    /// Load a catalog from disk.
    ///
    /// Returns `Ok(None)` when the file does not exist yet.
    /// Returns `Err(CacheError::Json)` for corrupt data.
    pub fn load(&self, source_id: &str) -> Result<Option<Catalog>, CacheError> {
        let path = self.path_for(source_id);
        match fs::read(&path) {
            Ok(bytes) => {
                let catalog = serde_json::from_slice(&bytes)?;
                Ok(Some(catalog))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(CacheError::Io(e)),
        }
    }

    /// Persist a catalog to disk atomically.
    ///
    /// Writes to `<path>.tmp` in the same directory, then renames over the
    /// final path so the swap is atomic on POSIX systems (same filesystem).
    pub fn store(&self, catalog: &Catalog) -> Result<(), CacheError> {
        let path = self.path_for(&catalog.source_id);
        let parent = path
            .parent()
            .expect("path_for always produces a nested path");

        fs::create_dir_all(parent)?;

        let tmp_path = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(catalog)?;
        fs::write(&tmp_path, &bytes)?;
        fs::rename(&tmp_path, &path)?;

        Ok(())
    }

    /// Return the number of seconds since the cached file was last modified.
    ///
    /// Returns `None` if the file does not exist or the mtime cannot be read.
    pub fn age_seconds(&self, source_id: &str) -> Option<i64> {
        let path = self.path_for(source_id);
        let meta = fs::metadata(&path).ok()?;
        let mtime = meta.modified().ok()?;
        let elapsed = SystemTime::now().duration_since(mtime).ok()?;
        Some(elapsed.as_secs() as i64)
    }

    /// Return `true` if the cache entry is missing or older than `ttl_seconds`.
    pub fn is_stale(&self, source_id: &str, ttl_seconds: i64) -> bool {
        match self.age_seconds(source_id) {
            None => true,
            Some(age) => age > ttl_seconds,
        }
    }
}

/// Replace filesystem-unsafe characters in a source id with `_`.
///
/// Replaces `/`, `\`, `:`, `*`, `?`, `"`, `<`, `>`, `|`, and NUL.
fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use modelx_core::testkit::sample_catalog;
    use tempfile::tempdir;

    #[test]
    fn store_then_load_returns_equal_catalog() {
        let dir = tempdir().unwrap();
        let cache = Cache::with_dir(dir.path().to_path_buf());
        let catalog = sample_catalog();
        cache.store(&catalog).unwrap();
        let loaded = cache.load(&catalog.source_id).unwrap();
        assert_eq!(loaded, Some(catalog));
    }

    #[test]
    fn load_missing_source_returns_none() {
        let dir = tempdir().unwrap();
        let cache = Cache::with_dir(dir.path().to_path_buf());
        let result = cache.load("nonexistent-source").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn path_for_layout() {
        let dir = tempdir().unwrap();
        let cache = Cache::with_dir(dir.path().to_path_buf());
        let path = cache.path_for("my-source");
        assert_eq!(path, dir.path().join("sources").join("my-source.json"));
    }

    #[test]
    fn store_twice_load_reflects_second() {
        let dir = tempdir().unwrap();
        let cache = Cache::with_dir(dir.path().to_path_buf());

        let mut catalog1 = sample_catalog();
        catalog1.fetched_at = Some(1_000);
        cache.store(&catalog1).unwrap();

        let mut catalog2 = sample_catalog();
        catalog2.fetched_at = Some(2_000);
        cache.store(&catalog2).unwrap();

        let loaded = cache.load(&catalog2.source_id).unwrap().unwrap();
        assert_eq!(loaded.fetched_at, Some(2_000));
    }

    #[test]
    fn is_stale_missing_source_is_true() {
        let dir = tempdir().unwrap();
        let cache = Cache::with_dir(dir.path().to_path_buf());
        assert!(cache.is_stale("no-such-source", 3600));
    }

    #[test]
    fn age_seconds_after_store_is_small() {
        let dir = tempdir().unwrap();
        let cache = Cache::with_dir(dir.path().to_path_buf());
        let catalog = sample_catalog();
        cache.store(&catalog).unwrap();
        let age = cache.age_seconds(&catalog.source_id);
        assert!(age.is_some());
        // File was just written; age should be well under a minute.
        assert!(age.unwrap() < 60);
    }

    #[test]
    fn id_with_slash_is_sanitized() {
        let dir = tempdir().unwrap();
        let cache = Cache::with_dir(dir.path().to_path_buf());
        let path = cache.path_for("a/b");
        // The final file component should be "a_b.json", not a nested path.
        assert_eq!(path.file_name().unwrap(), "a_b.json");
        // And there should be no extra directory component between "sources" and the file.
        let parent = path.parent().unwrap();
        assert_eq!(parent.file_name().unwrap(), "sources");
    }

    #[test]
    fn store_and_load_sanitized_id() {
        let dir = tempdir().unwrap();
        let cache = Cache::with_dir(dir.path().to_path_buf());
        // Use a catalog whose source_id contains a slash.
        let mut catalog = sample_catalog();
        catalog.source_id = "vendor/source".to_string();
        cache.store(&catalog).unwrap();
        let loaded = cache.load("vendor/source").unwrap();
        assert_eq!(loaded, Some(catalog));
    }
}
