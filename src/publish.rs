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

use rayon::prelude::*;
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
    let mut files: Files = BTreeMap::new();
    let mut stack = vec![entry.to_path_buf()];
    let mut seen: Vec<PathBuf> = Vec::new();

    while let Some(at) = stack.pop() {
        let real =
            fs::canonicalize(&at).map_err(|e| format!("cannot read {}: {e}", at.display()))?;
        if seen.contains(&real) {
            continue;
        }
        seen.push(real.clone());
        let text = fs::read_to_string(&real)
            .map_err(|e| format!("cannot read {}: {e}", real.display()))?;
        let name = file_name(&real, &dir);
        files.insert(name.clone(), text.replace('\u{feff}', ""));

        // a LICENSE beside the file comes with it
        let license = real.parent().map(|p| p.join("LICENSE"));
        if let Some(license) = license.filter(|l| l.is_file()) {
            let text = fs::read_to_string(&license)
                .map_err(|e| format!("cannot read {}: {e}", license.display()))?;
            let name = file_name(&license, &dir);
            files.insert(name, text.replace('\u{feff}', ""));
        }

        // an import is relative to the file that makes it
        let here = real.parent().unwrap_or(&dir);
        for import in imports_of(&text) {
            let target = here.join(&import);
            if target.is_file() {
                stack.push(target);
            }
        }
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

/// file_name is the path a file takes inside the package.
fn file_name(real: &Path, dir: &Path) -> String {
    real.strip_prefix(dir)
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
    let entry = Path::new(file);
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

/// publish sends a package to the hub and names it.
pub fn publish(args: &[String]) -> Result<(), String> {
    let named = args
        .first()
        .ok_or("a package to publish: bendenv publish <name>@<version> <file.bend>")?;
    let file = args
        .get(1)
        .ok_or("the file to publish: bendenv publish <name>@<version> <file.bend>")?;
    let (name, version) = named
        .split_once('@')
        .ok_or("a package as <name>@<version>: four numbers like 0.1.0.0")?;
    if !hub::valid_name(name) {
        return Err(format!("a package name of a-z, 0-9 and -: {name}"));
    }
    let version = crate::version::Version::parse(version)
        .ok_or_else(|| format!("a version of four numbers: {version}"))?;

    let entry = Path::new(file);
    if !entry.is_file() {
        return Err(format!("no such file: {file}"));
    }
    let (files, _) = pkg_files(entry)?;
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
    println!("import {name}@{version}/<entry>.bend as <Name>");
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
    let pre = format!("{hash} ");
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let found: Vec<u64> = (0..threads)
        .into_par_iter()
        .map(|from| search(&pre, lim, from as u64, threads as u64))
        .filter_map(|hit| hit)
        .collect();
    found
        .into_iter()
        .min()
        .ok_or_else(|| "no nonce was found".to_string())
}

/// search looks for a nonce, stepping over the ones the other threads
/// are looking at.
fn search(pre: &str, lim: u64, from: u64, step: u64) -> Option<u64> {
    let mut n = from;
    loop {
        if under(pre, n, lim) {
            return Some(n);
        }
        n += step;
    }
}

/// under says whether the digest of the prefix and the nonce falls under
/// the limit: the first 53 bits of the digest, read as a number.
fn under(pre: &str, nonce: u64, lim: u64) -> bool {
    let digest = hub::sha256(&format!("{pre}{nonce}"));
    let bytes = digest.as_bytes();
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
