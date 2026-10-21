//! Publishing a package to the hub.
//!
//! A package is a directory: the entry file, the files it imports, the
//! foreign code it names, and the LICENSE beside each. Its hash is the
//! digest of that set, so the same bytes always give the same hash and
//! a published version never moves.
//!
//! The hub asks for a proof of work before it accepts a package: a
//! nonce whose digest falls under a limit the hub sets, so publishing
//! costs something and cannot be flooded.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::hub;

/// the files of a package, by the path they take inside it
pub type Files = BTreeMap<String, String>;

use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
struct Pow {
    pow: f64,
}

/// hash_of is the content hash of a set of files: the digest of their
/// digests, one "<digest> <path>" per line, in path order.
pub fn hash_of(files: &Files) -> String {
    let mut lines = String::new();
    for (path, text) in files {
        lines.push_str(&hub::sha256(text));
        lines.push(' ');
        lines.push_str(path);
        lines.push('\n');
    }
    let digest = hub::sha256(&lines);
    format!("0x{}", &digest[..32])
}

/// pkg_files collects the files of a package: the entry file, the files
/// it imports, the foreign code it names, and the LICENSE beside each.
pub fn pkg_files(entry: &Path) -> Result<(Files, String), String> {
    // the directory the package's paths are relative to: the entry's,
    // canonical, so that a file's name inside the package is the same
    // whoever reads it
    let parent = match entry.parent() {
        Some(p) if p.as_os_str().is_empty() => Path::new("."),
        Some(p) => p,
        None => Path::new("."),
    };
    let dir = parent
        .canonicalize()
        .map_err(|e| format!("cannot read {}: {e}", parent.display()))?;
    // the files of the package, gathered first and named after: the root
    // of the package is the directory they share, so two files of the
    // same name in different directories stay two files.
    let mut stack = vec![entry.to_path_buf()];
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut found: Vec<PathBuf> = Vec::new();

    while let Some(at) = stack.pop() {
        let real =
            fs::canonicalize(&at).map_err(|e| format!("cannot read {}: {e}", at.display()))?;
        if seen.contains(&real) {
            continue;
        }
        seen.push(real.clone());
        found.push(real.clone());

        // a LICENSE beside the file comes with it
        if let Some(license) = real
            .parent()
            .map(|p| p.join("LICENSE"))
            .filter(|l| l.is_file())
        {
            let real = fs::canonicalize(&license)
                .map_err(|e| format!("cannot read {}: {e}", license.display()))?;
            if !seen.contains(&real) {
                seen.push(real.clone());
                found.push(real);
            }
        }

        let text = fs::read_to_string(&real)
            .map_err(|e| format!("cannot read {}: {e}", real.display()))?;
        // an import is relative to the file that makes it
        let here = real.parent().unwrap_or(&dir);
        for import in imports_of(&text) {
            let target = here.join(&import);
            if target.is_file() {
                stack.push(target);
            }
        }
    }

    let root = common_root(&found, &dir);
    let mut files: Files = BTreeMap::new();
    for real in &found {
        let text =
            fs::read_to_string(real).map_err(|e| format!("cannot read {}: {e}", real.display()))?;
        let name = file_name(real, &root);
        files.insert(name, text.replace('\u{feff}', ""));
    }

    let name = entry
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    if name.is_empty() {
        return Err("a published file needs a name before .bend".to_string());
    }
    Ok((files, name))
}

/// common_root is the directory the package's files share: the entry's
/// directory, climbed as deep as the files go above it.
fn common_root(files: &[PathBuf], dir: &Path) -> PathBuf {
    let mut root = dir.to_path_buf();
    'climb: loop {
        for f in files {
            if !f.starts_with(&root) {
                match root.parent() {
                    Some(up) => {
                        root = up.to_path_buf();
                        continue 'climb;
                    }
                    None => return PathBuf::from("/"),
                }
            }
        }
        return root;
    }
}

/// file_name is the path a file takes inside the package: the path from
/// the package's root. A file in a sibling directory keeps its
/// directory, so two files of the same name stay two files.
fn file_name(real: &Path, root: &Path) -> String {
    real.strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| {
            real.file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default()
        })
}

/// imports_of reads the paths a source file imports.
fn imports_of(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.starts_with("import") {
            break;
        }
        let rest = line[6..].trim_start();
        let path: String = rest
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '#')
            .collect();
        if path.is_empty() || path == "Base" {
            continue;
        }
        if path.starts_with('.') || path.ends_with(".c") || path.ends_with(".js") {
            out.push(path);
        }
    }
    out
}

/// hash_cmd prints the hash a package would publish under, and how many
/// files it would carry. It publishes nothing.
pub fn hash_cmd(args: &[String]) -> Result<(), String> {
    let file = args
        .first()
        .ok_or("the file to hash: bendenv hash <file.bend>")?;
    let entry = Path::new(&file);
    if !entry.is_file() {
        return Err(format!("no such file: {file}"));
    }
    let (files, name) = pkg_files(entry)?;
    let bytes: usize = files.values().map(|t| t.len()).sum();
    let hash = hash_of(&files);
    println!("{name}: {hash}");
    println!("{} files, {bytes} bytes", files.len());
    for path in files.keys() {
        println!("  {path}");
    }
    Ok(())
}

/// The hub takes a set of ".bend, .c and .js" files and a LICENSE, and
/// nothing else: a file of any other name is refused with a 400. The
/// check is here so the answer comes before the proof of work, which
/// costs seconds.
pub fn allowed(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    if name == "LICENSE" || name == "LICENCE" {
        return true;
    }
    [".bend", ".c", ".js"].iter().any(|e| name.ends_with(e))
}

/// check_allowed says what the hub would refuse.
pub fn check_allowed(files: &Files) -> Result<(), String> {
    let bad: Vec<&String> = files.keys().filter(|p| !allowed(p)).collect();
    if bad.is_empty() {
        return Ok(());
    }
    Err(format!(
        "the hub takes .bend, .c and .js files and a LICENSE, and nothing else; {} of the {} files are not: {}",
        bad.len(),
        files.len(),
        bad.iter()
            .take(6)
            .map(|p| p.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// JUNK are the directories a published source tree leaves out.
const JUNK: [&str; 6] = [".git", "target", "node_modules", ".bend", ".ez", "dist"];

/// dir_files collects the files of a directory: every file but the ones
/// that are build output or another package's cache.
///
/// The hub's guide says a package is a set of ".bend", ".c and .js" files
/// and a LICENSE, and that is what the compiler publishes. A source tree
/// is more than that, and publishing one is how a tool that is not a Bend
/// program can still be found on the hub: the bytes are there, the hash is
/// the same, and a reader can fetch them. It cannot be imported — the
/// loader reads .bend files and nothing else — so a package like this is
/// a source archive, not a library.
pub fn dir_files(dir: &Path) -> Result<Files, String> {
    let mut out: Files = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        let Ok(entries) = fs::read_dir(&at) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if JUNK.contains(&name.as_str()) {
                    continue;
                }
                stack.push(path);
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let rel = path
                .strip_prefix(dir)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| name.clone());
            out.insert(rel, text.replace('\u{feff}', ""));
        }
    }
    if out.is_empty() {
        return Err(format!("{} holds no files", dir.display()));
    }
    Ok(out)
}

/// the_entry is the file a package is read through: main.bend when there
/// is one, else the first file, in path order.
pub fn the_entry(files: &Files) -> String {
    for want in ["main.bend", "README.md", "readme.md"] {
        if files.contains_key(want) {
            return want.to_string();
        }
    }
    files
        .keys()
        .find(|p| p.ends_with(".bend"))
        .cloned()
        .unwrap_or_else(|| files.keys().next().cloned().unwrap_or_default())
}

/// the_description is the comment on the first line of the entry file:
/// the hub shows it under the package and under its name.
pub fn the_description(files: &Files, entry: &str) -> String {
    let Some(text) = files.get(entry) else {
        return String::new();
    };
    let Some(line) = text.lines().next() else {
        return String::new();
    };
    let line = line.trim();
    if !line.starts_with('#') {
        return String::new();
    }
    line.trim_start_matches('#').trim().to_string()
}

/// publish_dir sends a source tree to the hub, under a name when there is
/// one and by its hash when there is not.
pub fn publish_dir(args: &[String]) -> Result<(), String> {
    let dir = args
        .first()
        .ok_or("the directory to publish: bendenv publish <dir>")?;
    let at = Path::new(dir);
    if !at.is_dir() {
        return Err(format!("no such directory: {dir}"));
    }
    let files = dir_files(at)?;
    // the second argument names the file the package is read through,
    // and so where the description comes from
    let entry = match args.get(1) {
        Some(want) => {
            if !files.contains_key(want) {
                return Err(format!("{dir} holds no {want}"));
            }
            want.clone()
        }
        None => the_entry(&files),
    };
    let description = the_description(&files, &entry);
    check_allowed(&files)?;
    let bytes: usize = files.values().map(|t| t.len()).sum();
    let hash = hash_of(&files);
    println!("{}: {hash}", at.display());
    println!("{} files, {bytes} bytes, entry {entry}", files.len());
    if !description.is_empty() {
        println!("description: {description}");
    }
    let nonce = mine(&hash, bytes)?;
    let body = serde_json::json!({ "files": files, "nonce": nonce });
    let url = hub::hub_url();
    let res = ureq::post(&url)
        .header(
            "user-agent",
            &format!("bendenv/{}", env!("CARGO_PKG_VERSION")),
        )
        .send_json(&body)
        .map_err(|e| format!("the hub could not be reached ({url}): {e}"))?;
    if !res.status().is_success() {
        return Err(format!("the hub answered {}", res.status()));
    }
    let got = res.into_body().read_to_string().unwrap_or_default();
    let got = got.trim().to_string();
    if got != hash {
        return Err(format!("the hub answered {got} and not {hash}"));
    }
    println!("published {hash}");
    println!("import {hash}/{entry} as Name");
    Ok(())
}

/// publish sends a package to the hub and names it.
pub fn publish(args: &[String]) -> Result<(), String> {
    // one argument that names a file: the package goes up by its hash alone
    let (named, file) = match args.len() {
        1 if args[0].ends_with(".bend") => (None, args[0].clone()),
        1 => {
            return Err(
                "a package to publish: bendenv publish <name>@<version> <file.bend>".to_string(),
            );
        }
        _ => (Some(args[0].clone()), args[1].clone()),
    };
    let (name, version) = match named {
        None => (String::new(), String::new()),
        Some(named) => {
            let (name, version) = named
                .split_once('@')
                .ok_or("a package as <name>@<version>: four numbers like 0.1.0.0")?;
            (name.to_string(), version.to_string())
        }
    };
    if !name.is_empty() && !hub::valid_name(&name) {
        return Err(format!("a package name of a-z, 0-9 and -: {name}"));
    }
    if !version.is_empty() && crate::version::Version::parse(&version).is_none() {
        return Err(format!("a version of four numbers: {version}"));
    }

    let entry = Path::new(&file);
    if !entry.is_file() {
        return Err(format!("no such file: {file}"));
    }
    let (files, entry_name) = pkg_files(entry)?;
    check_allowed(&files)?;
    let bytes: usize = files.values().map(|t| t.len()).sum();
    let hash = hash_of(&files);
    let nonce = mine(&hash, bytes)?;

    let body = serde_json::json!({ "files": files, "nonce": nonce });
    let url = hub::hub_url();
    let res = ureq::post(&url)
        .header(
            "user-agent",
            &format!("bendenv/{}", env!("CARGO_PKG_VERSION")),
        )
        .send_json(&body)
        .map_err(|e| format!("the hub could not be reached ({url}): {e}"))?;
    if !res.status().is_success() {
        return Err(format!("the hub answered {} for {hash}", res.status()));
    }
    let got = res.into_body().read_to_string().unwrap_or_default();
    let got = got.trim().to_string();
    if got != hash {
        return Err(format!("the hub answered {got} and not {hash}"));
    }
    println!("published {hash} ({bytes} bytes)");
    if name.is_empty() {
        println!("import {hash}/{entry_name}.bend as <Name>");
        println!("name it with: bend link <name>@<version> {hash}");
    } else {
        println!("import {name}@{version}/{entry_name}.bend as <Name>");
    }
    Ok(())
}

/// mine finds a nonce whose digest falls under the hub's limit. The work
/// is split over the cores, the way the compiler's own publisher does it.
fn mine(hash: &str, bytes: usize) -> Result<u64, String> {
    let url = format!("{}/pow.json", hub::hub_url());
    let res = ureq::get(&url)
        .header(
            "user-agent",
            &format!("bendenv/{}", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .map_err(|e| format!("the hub could not be reached ({url}): {e}"))?;
    let got: Pow = res
        .into_body()
        .read_json()
        .map_err(|e| format!("the hub did not answer with a difficulty: {e}"))?;
    let lim = (2f64.powi(53) / (got.pow * (bytes as f64 / 262_144.0).max(1.0))) as u64;
    eprintln!(
        "mining a proof of work: difficulty {}, limit {}, {} cores",
        got.pow,
        lim,
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    );
    let started = std::time::Instant::now();
    let pre = format!("{hash} ");
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    // the first worker that finds one ends the search, the way the
    // compiler's own publisher takes the first answer. Each worker walks
    // its own arithmetic sequence of nonces: the pool of threads is the
    // one that splits the work, not an iterator over the nonces.
    let found = std::sync::atomic::AtomicU64::new(u64::MAX);
    let done = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|scope| {
        for k in 0..threads {
            let found = &found;
            let done = &done;
            let pre = pre.as_str();
            scope.spawn(move || {
                let mut n = k as u64;
                let step = threads as u64;
                while !done.load(std::sync::atomic::Ordering::Relaxed) {
                    if under(pre, n, lim) {
                        found.store(n, std::sync::atomic::Ordering::Relaxed);
                        done.store(true, std::sync::atomic::Ordering::Relaxed);
                        return;
                    }
                    n += step;
                }
            });
        }
    });
    let nonce = found.load(std::sync::atomic::Ordering::Relaxed);
    if nonce == u64::MAX {
        return Err("no nonce was found".to_string());
    }
    eprintln!("mined in {:?}", started.elapsed());
    Ok(nonce)
}

/// under says whether the digest of the prefix and the nonce falls under
/// the limit: the first 53 bits of the digest, read as a number.
fn under(pre: &str, nonce: u64, lim: u64) -> bool {
    let bytes = hub::sha256_bytes(&format!("{pre}{nonce}"));
    let high = (bytes[0] as u64) << 24
        | (bytes[1] as u64) << 16
        | (bytes[2] as u64) << 8
        | bytes[3] as u64;
    let low = ((bytes[4] as u64) << 24
        | (bytes[5] as u64) << 16
        | (bytes[6] as u64) << 8
        | bytes[7] as u64)
        >> 11;
    high * 2_097_152 + low < lim
}

#[cfg(test)]
mod tests {
    use super::*;

    /// the value the compiler's own publisher computes for a nonce, from
    /// its JavaScript: the first 53 bits of the digest, read as a number
    fn under_value(pre: &str, nonce: u64) -> u64 {
        let bytes = hub::sha256_bytes(&format!("{pre}{nonce}"));
        let high = (bytes[0] as u64) << 24
            | (bytes[1] as u64) << 16
            | (bytes[2] as u64) << 8
            | bytes[3] as u64;
        let low = ((bytes[4] as u64) << 24
            | (bytes[5] as u64) << 16
            | (bytes[6] as u64) << 8
            | bytes[7] as u64)
            >> 11;
        high * 2_097_152 + low
    }

    #[test]
    fn the_proof_of_work_reads_the_same_bits_the_compiler_reads() {
        // the values the compiler's JavaScript gives for these nonces
        let pre = "0xdeadbeef ";
        let want = [
            3666356130330830,
            3973388712586749,
            5482713495527784,
            8138382194172438,
            6143550805510620,
        ];
        for (n, w) in want.iter().enumerate() {
            assert_eq!(under_value(pre, n as u64), *w, "nonce {n}");
        }
    }

    #[test]
    fn the_hub_takes_bend_c_and_js_and_a_license_and_nothing_else() {
        // the hub's own words: "A package is a set of .bend, .c and .js
        // files, and a LICENSE, named by the hash of its content"
        for good in ["main.bend", "effs/print.c", "effs/print.js", "LICENSE"] {
            assert!(allowed(good), "{good} should be allowed");
        }
        for bad in [
            "src/main.rs",
            "README.md",
            "Cargo.toml",
            "Makefile",
            "conf.toml",
        ] {
            assert!(!allowed(bad), "{bad} should be refused");
        }
        let mut files: Files = BTreeMap::new();
        files.insert(
            "main.bend".to_string(),
            "def main() -> U32:\n  0\n".to_string(),
        );
        assert!(check_allowed(&files).is_ok());
        files.insert("src/main.rs".to_string(), "fn main() {}".to_string());
        let err = check_allowed(&files).unwrap_err();
        assert!(err.contains("src/main.rs"), "{err}");
    }

    #[test]
    fn the_limit_is_the_one_the_compiler_computes() {
        // 2^53 / (pow * max(1, bytes / 262144))
        let pow = 140000000.0_f64;
        let bytes = 3000_usize;
        let lim = (2f64.powi(53) / (pow * (bytes as f64 / 262_144.0).max(1.0))) as u64;
        assert_eq!(lim, 64_337_137);
    }
}
