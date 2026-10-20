//! The global cache.
//!
//! It is the same idea uv uses: one content-addressed store of every
//! file the hub has served, kept out of the project, and the project's
//! environment is made of hard links into it. A second project that
//! wants the same file downloads nothing, and an install writes no new
//! bytes at all.
//!
//! Two kinds of thing live here, and the difference matters. What a
//! package's hash names never changes — the files, the manifest, the
//! imports those files carry — so it is cached for good. What a
//! package's *name* names does change, because a new version can appear
//! at any moment, so that is cached for a while and then asked again.

use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::hub;

/// How long a package's page is kept before it is asked for again.
const PAGE_TTL: Duration = Duration::from_secs(300);

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

// The pages of the hub
// ===================

/// pages_dir holds the hub's answers that move: a package's page, with
/// the time it was read.
fn pages_dir() -> PathBuf {
    cache_dir().join("pages")
}

/// page reads a cached page, if it is still fresh.
fn page(name: &str) -> Option<String> {
    let at = pages_dir().join(name);
    let text = fs::read_to_string(&at).ok()?;
    let (stamp, body) = text.split_once('\n')?;
    let age = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .checked_sub(Duration::from_millis(stamp.parse().ok()?))
        .unwrap_or_default();
    if age > PAGE_TTL {
        return None;
    }
    Some(body.to_string())
}

/// write_page caches a page with the time it was read.
fn write_page(name: &str, body: &str) {
    let dir = pages_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let at = dir.join(name);
    let _ = fs::write(&at, format!("{now}\n{body}"));
}

/// info returns a package's page, from the cache when it is fresh and
/// from the hub when it is not.
pub fn info(name: &str) -> Result<hub::PkgInfo, String> {
    if let Some(got) = page(&format!("name-{name}"))
        .and_then(|text| serde_json::from_str::<hub::PkgInfo>(&text).ok())
    {
        return Ok(got);
    }
    let got = hub::info(name)?;
    if let Ok(text) = serde_json::to_string(&got) {
        write_page(&format!("name-{name}"), &text);
    }
    Ok(got)
}

/// hash_of returns the hash a pinned name and version carry, from the
/// cache when the page is fresh and from the hub when it is not.
pub fn hash_of(pinned: &str) -> Result<String, String> {
    if let Some(text) = page(&format!("hash-{pinned}")) {
        let got = text.trim().to_string();
        if hub::valid_hash(&got) {
            return Ok(got);
        }
    }
    let got = hub::hash_of(pinned)?;
    write_page(&format!("hash-{pinned}"), &got);
    Ok(got)
}

// What a hash names
// ================

/// manifest returns the files of a package. A hash names the same files
/// for ever, so this is cached for good.
pub fn manifest(hash: &str) -> Result<Vec<hub::HubFile>, String> {
    let at = cache_dir().join("manifests").join(hash);
    if let Ok(text) = fs::read_to_string(&at) {
        let mut out = Vec::new();
        for line in text.lines() {
            let Some((sha, path)) = line.split_once(' ') else {
                continue;
            };
            out.push(hub::HubFile {
                sha: sha.to_string(),
                path: path.to_string(),
            });
        }
        if !out.is_empty() {
            return Ok(out);
        }
    }
    let got = hub::manifest(hash)?;
    let mut text = String::new();
    for f in &got {
        text.push_str(&f.sha);
        text.push(' ');
        text.push_str(&f.path);
        text.push('\n');
    }
    let dir = cache_dir().join("manifests");
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(dir.join(hash), text);
    Ok(got)
}

/// deps returns the packages a package's sources import. A hash names
/// the same sources for ever, so this is cached for good.
pub fn deps(hash: &str) -> Result<Vec<String>, String> {
    let at = cache_dir().join("deps2").join(hash);
    if let Ok(text) = fs::read_to_string(&at) {
        let out: Vec<String> = text
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if !out.is_empty() {
            return Ok(out);
        }
    }
    let files = manifest(hash)?;
    // the sources of a package, read at once: a package with two hundred
    // files is two hundred round trips, and they do not have to be one
    // after the other
    let sources: Vec<Result<String, String>> = files
        .par_iter()
        .filter(|f| f.path.ends_with(".bend"))
        .map(|f| hub::file(hash, &f.path, &f.sha))
        .collect();
    let mut out: Vec<String> = Vec::new();
    for text in sources.into_iter().collect::<Result<Vec<_>, _>>()? {
        for p in crate::scan::imports(&text) {
            let dep = p.to_string();
            if !out.contains(&dep) {
                out.push(dep);
            }
        }
    }
    out.sort();
    let dir = cache_dir().join("deps2");
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(dir.join(hash), out.join("\n"));
    Ok(out)
}

/// constructors returns the names of the constructors a package
/// declares. A hash names the same sources for ever, so this is cached
/// for good.
///
/// The constructors, not the types: a Bend constructor is a name in the
/// namespace of its package, so `Text` in one package and `Text` in
/// another are two names that cannot both be in scope. That is the
/// clash that breaks a program, not the type names.
pub fn constructors(hash: &str) -> Result<Vec<String>, String> {
    let at = cache_dir().join("ctors2").join(hash);
    if let Ok(text) = fs::read_to_string(&at) {
        let out: Vec<String> = text
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if !out.is_empty() {
            return Ok(out);
        }
    }
    let files = manifest(hash)?;
    let sources: Vec<Result<String, String>> = files
        .par_iter()
        .filter(|f| f.path.ends_with(".bend"))
        .map(|f| hub::file(hash, &f.path, &f.sha))
        .collect();
    let mut out: Vec<String> = Vec::new();
    for text in sources.into_iter().collect::<Result<Vec<_>, _>>()? {
        for line in text.lines() {
            // a constructor is written like this, indented under its type:
            //   Text{text: String}
            let Some(rest) = line.strip_prefix("  ") else {
                continue;
            };
            if line.starts_with("   ") || rest.starts_with(' ') {
                continue;
            }
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if name.is_empty() {
                continue;
            }
            if !name.chars().next().is_some_and(|c| c.is_uppercase()) {
                continue;
            }
            if !rest[name.len()..].starts_with('{') {
                continue;
            }
            if !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out.sort();
    let dir = cache_dir().join("ctors2");
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(dir.join(hash), out.join("\n"));
    Ok(out)
}

/// base_constructors returns the names of the constructors Base declares.
///
/// Base is not on the hub: it comes with the compiler, and it moves. A
/// constructor Base gains can collide with one a published package
/// declares, and then every program that imports both refuses to compile.
/// The names are read from the installed Base, when it can be found.
pub fn base_constructors() -> Vec<String> {
    let Some(text) = base_bend() else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        // a constructor is written like this, indented under its type:
        //   Text{text: String}
        let Some(rest) = line.strip_prefix("  ") else {
            continue;
        };
        if line.starts_with("   ") || rest.starts_with(' ') {
            continue;
        }
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }
        if !name.chars().next().is_some_and(|c| c.is_uppercase()) {
            continue;
        }
        if !rest[name.len()..].starts_with('{') {
            continue;
        }
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out.sort();
    out
}

/// base_bend finds the Base source that comes with the compiler.
fn base_bend() -> Option<String> {
    let mut ats: Vec<PathBuf> = Vec::new();
    if let Ok(at) = std::env::var("BEND_DIR") {
        ats.push(PathBuf::from(at).join("base.bend"));
    }
    if let Ok(home) = std::env::var("HOME") {
        ats.push(
            PathBuf::from(&home)
                .join(".bend")
                .join("bend2")
                .join("base.bend"),
        );
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut up = exe.parent().map(|p| p.to_path_buf());
        while let Some(at) = up {
            ats.push(at.join("bend2").join("base.bend"));
            up = at.parent().map(|p| p.to_path_buf());
        }
    }
    for at in ats {
        if let Ok(text) = fs::read_to_string(&at) {
            return Some(text);
        }
    }
    None
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
    for sub in [
        "pages",
        "manifests",
        "deps",
        "types",
        "deps2",
        "types2",
        "ctors2",
    ] {
        let dir = cache_dir().join(sub);
        if !dir.is_dir() {
            continue;
        }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let _ = fs::remove_file(entry.path());
                n += 1;
            }
        }
    }
    Ok(n)
}

/// size is how many bytes the store holds.
pub fn size() -> u64 {
    let mut n = 0;
    for sub in [
        "files",
        "pages",
        "manifests",
        "deps",
        "types",
        "deps2",
        "types2",
    ] {
        let dir = cache_dir().join(sub);
        if !dir.is_dir() {
            continue;
        }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    n += meta.len();
                }
            }
        }
    }
    n
}
