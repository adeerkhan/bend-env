//! The environment: a directory the loader reads as its package library.
//!
//! It is the shape bend already uses for its own cache, ~/.bend/lib: a
//! "names" directory that maps a pinned name to a hash, and one
//! directory per hash holding the files of that package. Setting
//! BEND_LIB to the environment makes the loader read it and nothing
//! else, so an environment is a package set, pinned and verified.
//!
//! The files are hard links into the global cache, so an install writes
//! no new bytes and a second project that wants the same package pays
//! nothing.

use std::fs;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::cache;
use crate::hub;
use crate::lock::Lock;

/// ENV_DIR is where the environment of a project lives.
pub const ENV_DIR: &str = ".bend/env";

#[derive(Debug, Default)]
pub struct SyncReport {
    pub packages: Vec<String>,
    /// the files that were fetched from the hub
    pub fetched: usize,
    /// the files that were installed out of the cache
    pub installed: usize,
    /// the files that were already in the environment and matched
    pub reused: usize,
    /// the files that were linked rather than copied
    pub linked: usize,
}

/// env_path is the environment of a project directory.
pub fn env_path(root: &Path) -> PathBuf {
    root.join(ENV_DIR)
}

/// sync materializes a lockfile into an environment: every package's
/// files, fetched into the global cache in parallel and linked into
/// place, and the names that point the loader at them.
pub fn sync(env: &Path, l: &Lock) -> Result<SyncReport, String> {
    fs::create_dir_all(env.join("names"))
        .map_err(|e| format!("cannot make {}: {e}", env.join("names").display()))?;
    let mut report = SyncReport::default();

    for pkg in &l.packages {
        let dir = env.join(&pkg.hash);
        fs::create_dir_all(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;

        // fetch the files that are not in the cache yet, in parallel
        let missing: Vec<&crate::lock::LockedFile> =
            pkg.files.iter().filter(|f| !cache::has(&f.sha)).collect();
        let results: Vec<Result<(), String>> = missing
            .par_iter()
            .map(|f| cache::fetch(&pkg.hash, &f.path, &f.sha))
            .collect();
        for r in results {
            r?;
        }
        report.fetched += missing.len();

        for f in &pkg.files {
            let at = dir.join(&f.path);
            if let Some(parent) = at.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot make {}: {e}", parent.display()))?;
            }
            if at.is_file() {
                let got = fs::read_to_string(&at).unwrap_or_default();
                if hub::sha256(&got) == f.sha {
                    report.reused += 1;
                    continue;
                }
            }
            let src = cache::path(&f.sha);
            if !src.is_file() {
                return Err(format!(
                    "the cache has no {} for {}/{}",
                    f.sha, pkg.hash, f.path
                ));
            }
            report.installed += 1;
            match fs::hard_link(&src, &at) {
                Ok(()) => report.linked += 1,
                Err(_) => {
                    // a file system that will not link: copy instead
                    fs::copy(&src, &at)
                        .map_err(|e| format!("cannot install {}: {e}", at.display()))?;
                }
            }
        }

        write_name(env, &format!("{}@{}", pkg.name, pkg.version), &pkg.hash)?;
        report
            .packages
            .push(format!("{}@{}", pkg.name, pkg.version));
    }
    Ok(report)
}

/// write_name points a pinned name at a hash, the way the loader reads it.
pub fn write_name(env: &Path, pinned: &str, hash: &str) -> Result<(), String> {
    let at = env.join("names").join(pinned);
    fs::create_dir_all(env.join("names"))
        .map_err(|e| format!("cannot make {}: {e}", env.join("names").display()))?;
    fs::write(&at, format!("{hash}\n")).map_err(|e| format!("cannot write {}: {e}", at.display()))
}

/// check verifies an environment against a lockfile: every package's
/// files are there and match the hash the hub published, and every name
/// points at the right hash. It reads the lock alone, so it needs no
/// network.
pub fn check(env: &Path, l: &Lock) -> Vec<String> {
    let mut bad = Vec::new();
    for pkg in &l.packages {
        let dir = env.join(&pkg.hash);
        if !dir.is_dir() {
            bad.push(format!(
                "{}@{} is not in the environment",
                pkg.name, pkg.version
            ));
            continue;
        }
        for f in &pkg.files {
            let at = dir.join(&f.path);
            if !at.is_file() {
                bad.push(format!("{}/{} is missing", pkg.hash, f.path));
                continue;
            }
            let got = fs::read_to_string(&at).unwrap_or_default();
            if hub::sha256(&got) != f.sha {
                bad.push(format!(
                    "{}/{} does not match the hash the hub published",
                    pkg.hash, f.path
                ));
            }
        }
        let name_at = env
            .join("names")
            .join(format!("{}@{}", pkg.name, pkg.version));
        if !name_at.is_file() {
            bad.push(format!(
                "{}@{} has no name in the environment",
                pkg.name, pkg.version
            ));
        } else if fs::read_to_string(&name_at).unwrap_or_default().trim() != pkg.hash {
            bad.push(format!(
                "{}@{} points at another hash",
                pkg.name, pkg.version
            ));
        }
    }
    bad
}

/// purge deletes an environment.
pub fn purge(env: &Path) -> Result<(), String> {
    if env.is_dir() {
        fs::remove_dir_all(env).map_err(|e| format!("cannot remove {}: {e}", env.display()))?;
    }
    Ok(())
}
