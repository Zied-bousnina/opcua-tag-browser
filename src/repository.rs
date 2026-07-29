//! Persisting a scanned tag list so you do not rescan on every start.

use crate::error::{Error, Result};
use crate::tag::PlcTag;
use std::fs;
use std::path::{Path, PathBuf};

/// Stores and retrieves a cached tag list.
///
/// Implement this to back the cache with something other than a file, such as
/// a database or an object store.
pub trait TagRepository {
    /// Returns `true` if a cache is present.
    fn exists(&self) -> bool;
    /// Loads the cached tags.
    fn load(&self) -> Result<Vec<PlcTag>>;
    /// Replaces the cache with `tags`.
    fn save(&self, tags: &[PlcTag]) -> Result<()>;
}

/// A [`TagRepository`] backed by a pretty-printed JSON file.
#[derive(Debug, Clone)]
pub struct JsonFileTagRepository {
    path: PathBuf,
}

impl JsonFileTagRepository {
    /// Creates a repository over the given file path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Returns the file path this repository reads and writes.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl TagRepository for JsonFileTagRepository {
    fn exists(&self) -> bool {
        self.path.exists()
    }

    fn load(&self) -> Result<Vec<PlcTag>> {
        let content = fs::read_to_string(&self.path).map_err(|source| Error::CacheIo {
            path: self.path.clone(),
            source,
        })?;

        serde_json::from_str(&content).map_err(|source| Error::CacheFormat {
            path: self.path.clone(),
            source,
        })
    }

    fn save(&self, tags: &[PlcTag]) -> Result<()> {
        let json = serde_json::to_string_pretty(tags).map_err(|source| Error::CacheFormat {
            path: self.path.clone(),
            source,
        })?;

        fs::write(&self.path, json).map_err(|source| Error::CacheIo {
            path: self.path.clone(),
            source,
        })
    }
}