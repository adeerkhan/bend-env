//! The ledger of ez, the other package manager: `ez.toml`.
//!
//! ez's ledger is a record of every dependency a project has, by name and
//! by the hash of its content, with where it came from: a name on the hub,
//! or a git commit with a NAR hash. It is a *ledger*, not a manifest —
//! the versions are already decided.
//!
//! bend-env can read it. The hashes in it are the same content hashes
//! bend-env's lock holds, so a project that ez set up can be fetched,
//! checked and synced by bend-env, and bend-env can write a ledger that
//! ez reads back.
//!
//! The shape, from ez's own ledger/render:
//!
//! ```toml
//! [package]
//! name = "my-project"
//! entry = "main.bend"
//!
//! [deps.bend-ml-tensor-array]
//! hash = "0x0f64..."
//! hub = "bend-ml-tensor-array"
//! entry = "main.bend"
//!
//! [deps.something-from-git]
//! hash = "0x..."
//! git = "https://github.com/..."
//! rev = "abc123"
//! narHash = "sha256-..."
//! vendor = true
//!
//! [tools.some-cli]
//! git = "https://github.com/..."
//! entry = "main.bend"
//! bin = "cli"
//! ```

use std::collections::BTreeMap;

use crate::lock::{Lock, Locked};

/// A dependency of a ledger: a name, a hash, and where it came from.
#[derive(Clone, Debug)]
pub struct Dep {
    pub name: String,
    pub hash: String,
    /// the file the package is read through
    pub entry: String,
    /// the hub the package comes from, when it is a hub package
    pub hub: Option<String>,
    /// the git url, when the package comes from a repository
    pub git: Option<String>,
    /// the commit, when the package comes from a repository
    pub rev: Option<String>,
}

impl Dep {
    /// pinned is how a ledger names a dependency: a name and a version.
    pub fn pinned(&self) -> String {
        self.name.clone()
    }
}

/// A ledger: the package itself, its dependencies, and its tools.
#[derive(Clone, Debug, Default)]
pub struct Ledger {
    pub name: String,
    pub entry: String,
    pub bin: String,
    pub hub: String,
    pub publish_as: String,
    pub version: String,
    pub deps: Vec<Dep>,
    /// the tools: pinned CLIs, which are not packages and are not imported
    pub tools: Vec<BTreeMap<String, String>>,
}

/// read parses an `ez.toml`.
pub fn read(text: &str) -> Result<Ledger, String> {
    let got: toml::Value = toml::from_str(text).map_err(|e| format!("ez.toml is not TOML: {e}"))?;
    let mut led = Ledger::default();
    if let Some(pkg) = got.get("package").and_then(|v| v.as_table()) {
        led.name = text_of(pkg, "name");
        led.entry = text_of(pkg, "entry");
        led.bin = text_of(pkg, "bin");
        led.hub = text_of(pkg, "hub");
        led.publish_as = text_of(pkg, "publish-as");
        led.version = text_of(pkg, "version");
    }
    if let Some(deps) = got.get("deps").and_then(|v| v.as_table()) {
        for (name, body) in deps {
            let Some(t) = body.as_table() else { continue };
            led.deps.push(Dep {
                name: name.clone(),
                hash: text_of(t, "hash"),
                entry: text_of(t, "entry"),
                hub: opt_of(t, "hub"),
                git: opt_of(t, "git"),
                rev: opt_of(t, "rev"),
            });
        }
    }
    if let Some(tools) = got.get("tools").and_then(|v| v.as_table()) {
        for (name, body) in tools {
            let mut tool = BTreeMap::new();
            if let Some(t) = body.as_table() {
                for (k, v) in t {
                    if let Some(s) = v.as_str() {
                        tool.insert(k.clone(), s.to_string());
                    }
                }
            }
            led.tools.push(tool);
            let _ = name;
        }
    }
    Ok(led)
}

/// text_of reads a key of a table, or "" when it is not there.
fn text_of(t: &toml::value::Table, key: &str) -> String {
    t.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// opt_of reads a key of a table, or nothing when it is not there.
fn opt_of(t: &toml::value::Table, key: &str) -> Option<String> {
    t.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// to_lock turns a ledger into the lock bend-env resolves and syncs.
///
/// A ledger names its dependencies by hash, so the versions are not in
/// it: the hash is the identity. The lock records both, and the name a
/// dependency has on the hub is looked up when the lock is written, so
/// the two can be read back and forth.
pub fn to_lock(led: &Ledger) -> Lock {
    let mut packages = Vec::new();
    for dep in &led.deps {
        packages.push(Locked {
            name: dep.name.clone(),
            version: String::new(),
            hash: dep.hash.clone(),
            source: if dep.git.is_some() { "git" } else { "hub" }.to_string(),
            dependencies: Vec::new(),
            files: Vec::new(),
            constructors: Vec::new(),
        });
    }
    Lock {
        version: crate::lock::LOCK_VERSION,
        packages,
    }
}

/// write renders a lock as an ez ledger: the dependencies by name and by
/// the hash of their content, with the hub they come from.
pub fn write(lock: &Lock, name: &str, entry: &str) -> String {
    let mut out = String::new();
    out.push_str("[package]\n");
    out.push_str(&format!("name = \"{name}\"\n"));
    if !entry.is_empty() {
        out.push_str(&format!("entry = \"{entry}\"\n"));
    }
    for pkg in &lock.packages {
        if pkg.name.is_empty() {
            continue;
        }
        out.push_str(&format!("\n[deps.{}]\n", pkg.name));
        out.push_str(&format!("hash = \"{}\"\n", pkg.hash));
        if pkg.source == "hub" {
            out.push_str(&format!("hub = \"{}\"\n", pkg.name));
        }
    }
    out
}
