//! The lockfile: bend.lock.
//!
//! It holds every package the project needs, the exact version and the
//! content hash of each, the packages each one depends on, and the files
//! of each with their digests. The hashes are what make a run
//! reproducible: a published version never moves, and the hash is checked
//! against the bytes.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const LOCK_NAME: &str = "bend.lock";
pub const LOCK_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LockedFile {
    /// the SHA-256 the hub published for the file
    pub sha: String,
    /// the path of the file inside the package
    pub path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Locked {
    pub name: String,
    pub version: String,
    pub hash: String,
    #[serde(default = "hub_source")]
    pub source: String,
    /// the pinned dependencies of this package, "name@version" each
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// the files of the package, so the lock is the whole of it
    #[serde(default)]
    pub files: Vec<LockedFile>,
    /// the names of the constructors the package declares
    #[serde(default)]
    pub constructors: Vec<String>,
}

fn hub_source() -> String {
    "hub".to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lock {
    pub version: u32,
    #[serde(rename = "package")]
    pub packages: Vec<Locked>,
}

impl Lock {
    pub fn empty() -> Lock {
        Lock {
            version: LOCK_VERSION,
            packages: Vec::new(),
        }
    }

    /// read parses a lockfile.
    pub fn read(file: &Path) -> Result<Lock, String> {
        let text =
            fs::read_to_string(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
        let got: Lock = toml::from_str(&text)
            .map_err(|e| format!("{} is not a lockfile: {e}", file.display()))?;
        if got.version != LOCK_VERSION {
            return Err(format!(
                "{} has lock version {}, and this tool writes version {LOCK_VERSION}",
                file.display(),
                got.version
            ));
        }
        Ok(got)
    }

    /// write puts a lockfile back on disk, sorted so the diff is readable.
    pub fn write(&self, file: &Path) -> Result<(), String> {
        let mut packages = self.packages.clone();
        packages.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.version.cmp(&b.version)));
        let out = Lock {
            version: LOCK_VERSION,
            packages,
        };
        let text = toml::to_string_pretty(&out)
            .map_err(|e| format!("cannot write {}: {e}", file.display()))?;
        fs::write(file, text).map_err(|e| format!("cannot write {}: {e}", file.display()))
    }

    /// find returns the package of a name and version.
    pub fn find(&self, pinned: &str) -> Option<&Locked> {
        self.packages
            .iter()
            .find(|p| format!("{}@{}", p.name, p.version) == pinned)
    }

    /// collisions finds the type names two packages of the closure both
    /// declare. A Bend type belongs to the package that declares it, so
    /// two packages that declare `Mat` are two different `Mat`, and a
    /// program that mixes them does not compile.
    pub fn collisions(&self) -> Vec<(String, Vec<String>)> {
        let mut by_type: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for pkg in &self.packages {
            // a package named by its hash alone has no name to report
            let name = if pkg.name.is_empty() {
                pkg.hash.clone()
            } else {
                pkg.name.clone()
            };
            for t in &pkg.constructors {
                let list = by_type.entry(t.clone()).or_default();
                if !list.contains(&name) {
                    list.push(name.clone());
                }
            }
        }
        by_type
            .into_iter()
            .filter(|(_, names)| names.len() > 1)
            .map(|(name, mut names)| {
                names.sort();
                (name, names)
            })
            .collect()
    }

    /// skew finds the packages the closure holds at two versions. Two
    /// versions of one package sit side by side happily, but they are two
    /// different types, and the checker refuses to mix them.
    pub fn skew(&self) -> Vec<(String, Vec<String>)> {
        let mut by_name: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for pkg in &self.packages {
            by_name
                .entry(pkg.name.clone())
                .or_default()
                .push(pkg.version.clone());
        }
        by_name
            .into_iter()
            .filter(|(_, versions)| versions.len() > 1)
            .map(|(name, mut versions)| {
                versions.sort();
                (name, versions)
            })
            .collect()
    }
}
