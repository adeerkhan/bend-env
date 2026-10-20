//! The global cache.
//!
//! It is the same idea uv uses: one content-addressed store of every
//! file the hub has served, kept out of the project, and the project's
//! environment is made of hard links into it. A second project that
//! wants the same file downloads nothing, and an install writes no new
//! bytes at all.

use std::fs;
use std::path::{Path, PathBuf};

use crate::hub;

/// cache_dir is where the store lives: $BEND_CACHE, or ~/.cache/bendenv.
pub fn cache_dir() -> PathBuf {
    if let Ok(at) = std::env::var("BEND_CACHE") {
        return PathBuf::from(at);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    Path::new(&home).join(".cache").join("bendenv")
}

/// files_dir is the store of file contents, keyed by the SHA-256 the hub
/// published.
pub fn files_dir() -> PathBuf {
    cache_dir().join("files")
}

/// path is where a file with a given digest lives in the store.
pub fn path(sha: &str) -> PathBuf {
    files_dir().join(sha)
}

/// has says whether the store already holds a file.
pub fn has(sha: &str) -> bool {
    path(sha).is_file()
}

/// put writes a file into the store, under its own name, so the store
/// needs no index: the name of a file is its digest.
pub fn put(sha: &str, text: &str) -> Result<(), String> {
    let dir = files_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    let at = path(sha);
    if at.is_file() {
        return Ok(());
    }
    let tmp = dir.join(format!(".{sha}.tmp"));
    fs::write(&tmp, text).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, &at).map_err(|e| format!("cannot write {}: {e}", at.display()))?;
    Ok(())
}

/// fetch reads a file from the hub and puts it in the store.
pub fn fetch(hash: &str, path_in_package: &str, sha: &str) -> Result<(), String> {
    if has(sha) {
        return Ok(());
    }
    let text = hub::file(hash, path_in_package, sha)?;
    put(sha, &text)
}

/// clean empties the store.
pub fn clean() -> Result<usize, String> {
    let dir = files_dir();
    if !dir.is_dir() {
        return Ok(0);
    }
    let mut n = 0;
    for entry in fs::read_dir(&dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let _ = fs::remove_file(entry.path());
        n += 1;
    }
    Ok(n)
}

/// size is how many bytes the store holds.
pub fn size() -> u64 {
    let dir = files_dir();
    if !dir.is_dir() {
        return 0;
    }
    let mut n = 0;
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                n += meta.len();
            }
        }
    }
    n
}
