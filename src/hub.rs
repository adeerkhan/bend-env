//! The BendHub client.
//!
//! BendHub is the registry: it holds content-addressed packages, names a
//! hash with a name and a four-number version, and serves the files of a
//! package under that hash. Everything here verifies what it fetches
//! against the SHA-256 the hub published, so a package that changed under
//! us is an error and not a silent difference.

use serde::Deserialize;
use sha2::{Digest, Sha256};

pub fn hub_url() -> String {
    std::env::var("BEND_HUB").unwrap_or_else(|_| "https://hub.bend-lang.com".to_string())
}

/// The name a package may carry: a-z, 0-9 and -, as the hub's NAMED says.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A pinned reference: a name and a four-number version.
pub fn split_pinned(pinned: &str) -> Option<(&str, &str)> {
    let at = pinned.rfind('@')?;
    Some((&pinned[..at], &pinned[at + 1..]))
}

pub fn valid_hash(hash: &str) -> bool {
    hash.len() == 34
        && hash.starts_with("0x")
        && hash[2..]
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[derive(Clone, Debug, Deserialize, serde::Serialize)]
pub struct PkgVersion {
    pub version: String,
    pub hash: String,
    #[serde(default)]
    pub ts: u64,
}

#[derive(Clone, Debug, Deserialize, serde::Serialize)]
pub struct PkgLatest {
    pub version: String,
    pub hash: String,
    #[serde(default)]
    pub ts: u64,
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default)]
    pub entry: Option<String>,
}

#[derive(Clone, Debug, Deserialize, serde::Serialize)]
pub struct PkgInfo {
    pub name: String,
    #[serde(default)]
    pub owner_login: String,
    #[serde(default)]
    pub dependents: u64,
    #[serde(default)]
    pub latest: Option<PkgLatest>,
    pub versions: Vec<PkgVersion>,
}

#[derive(Clone, Debug)]
pub struct HubFile {
    /// the SHA-256 the hub published for the file
    pub sha: String,
    /// the path of the file inside the package
    pub path: String,
}

fn agent() -> String {
    let v = env!("CARGO_PKG_VERSION");
    format!("bendenv/{v}")
}

fn get(url: &str) -> Result<String, String> {
    let agent = agent();
    let res = ureq::get(url)
        .header("user-agent", &agent)
        .call()
        .map_err(|e| format!("the hub could not be reached ({url}): {e}"))?;
    res.into_body()
        .read_to_string()
        .map_err(|e| format!("the hub answered with something unreadable ({url}): {e}"))
}

/// info reads a package's page on the hub: its owner, its latest version
/// and every version it has published.
pub fn info(name: &str) -> Result<PkgInfo, String> {
    let url = format!("{}/name/{name}", hub_url());
    let res = ureq::get(&url)
        .header("user-agent", &agent())
        .call()
        .map_err(|e| format!("the hub could not be reached ({url}): {e}"))?;
    if res.status() == 404 {
        return Err(format!("no package named {name} on {}", hub_url()));
    }
    let got: PkgInfo = res
        .into_body()
        .read_json()
        .map_err(|e| format!("the hub did not answer with a package for {name}: {e}"))?;
    Ok(got)
}

/// hash_of reads the hash a pinned name and version carry.
pub fn hash_of(pinned: &str) -> Result<String, String> {
    let url = format!("{}/name/{pinned}", hub_url());
    let got = get(&url)?;
    let got = got.trim().to_string();
    if !valid_hash(&got) {
        return Err(format!("the hub did not answer with a hash for {pinned}"));
    }
    Ok(got)
}

/// manifest reads the files of a package: the SHA-256 of each file and
/// its path inside the package. The hub writes one "<sha> <path>" per line.
pub fn manifest(hash: &str) -> Result<Vec<HubFile>, String> {
    let url = format!("{}/{hash}/manifest", hub_url());
    let text = get(&url)?;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (sha, path) = line
            .split_once(' ')
            .ok_or_else(|| format!("a manifest line of {hash} is not '<sha> <path>': {line}"))?;
        out.push(HubFile {
            sha: sha.to_string(),
            path: path.to_string(),
        });
    }
    Ok(out)
}

/// file reads one file of a package and checks it against the SHA-256 the
/// manifest published. A file that does not match is refused.
pub fn file(hash: &str, path: &str, sha: &str) -> Result<String, String> {
    let url = format!("{}/{hash}/{path}", hub_url());
    let text = get(&url)?;
    if sha256(&text) != sha {
        return Err(format!(
            "{hash}/{path} does not match the hash the hub published"
        ));
    }
    Ok(text)
}

/// sha256 is the hex digest of a string.
pub fn sha256(data: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// sha256_bytes is the digest of a string, as the bytes it is: the proof
/// of work reads the digest's bits, not its hex.
pub fn sha256_bytes(data: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    hasher.finalize().into()
}
