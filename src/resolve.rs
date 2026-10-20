//! The resolver.
//!
//! It walks the dependencies of a project and writes down what it found:
//! every package, its exact version and its content hash, the packages
//! each one depends on, and the files of each.
//!
//! There is nothing to backtrack over. A Bend source names its
//! dependencies exactly ("bend-ml-tensor-array@0.1.14.0"), the loader
//! resolves a name and a version to a hash, and two versions of one
//! package live in two directories and never meet. So the closure of a
//! project is a set, not a choice: the resolver only has to walk it.

use std::collections::BTreeMap;

use crate::hub::{self, PkgInfo};
use crate::lock::{LOCK_VERSION, Lock, Locked, LockedFile};
use crate::scan::imports;
use crate::version::{Constraint, Version};

#[derive(Clone)]
pub struct Root {
    pub name: String,
    pub constraint: String,
}

/// resolve walks the dependencies of a project and returns the closure.
pub fn resolve(roots: &[Root]) -> Result<Lock, String> {
    let mut out: BTreeMap<String, Locked> = BTreeMap::new();
    let mut infos: BTreeMap<String, PkgInfo> = BTreeMap::new();
    let mut queue: Vec<Root> = roots.to_vec();

    while !queue.is_empty() {
        let root = queue.remove(0);
        let picked = pick(&root, &mut infos)?;
        let pinned = format!("{}@{}", picked.name, picked.version);
        if out.contains_key(&pinned) {
            continue;
        }

        let files = hub::manifest(&picked.hash)?;
        let mut deps: Vec<String> = Vec::new();
        for f in &files {
            if !f.path.ends_with(".bend") {
                continue;
            }
            let text = hub::file(&picked.hash, &f.path, &f.sha)?;
            for p in imports(&text) {
                let dep = p.to_string();
                if !deps.contains(&dep) {
                    deps.push(dep);
                }
            }
        }
        deps.sort();
        out.insert(
            pinned.clone(),
            Locked {
                name: picked.name.clone(),
                version: picked.version.clone(),
                hash: picked.hash.clone(),
                source: "hub".to_string(),
                dependencies: deps.clone(),
                files: files
                    .iter()
                    .map(|f| LockedFile {
                        sha: f.sha.clone(),
                        path: f.path.clone(),
                    })
                    .collect(),
            },
        );
        for dep in deps {
            if let Some(p) = split_pinned(&dep) {
                queue.push(Root {
                    name: p.0.to_string(),
                    constraint: p.1.to_string(),
                });
            }
        }
    }

    Ok(Lock {
        version: LOCK_VERSION,
        packages: out.into_values().collect(),
    })
}

struct Picked {
    name: String,
    version: String,
    hash: String,
}

/// pick chooses the version of a package a constraint asks for: the
/// highest one that satisfies it. An exact constraint reads the hash of
/// that version straight from the hub.
fn pick(root: &Root, infos: &mut BTreeMap<String, PkgInfo>) -> Result<Picked, String> {
    let want = Constraint::parse(&root.constraint)?;
    if let Some(exact) = want.exact() {
        let pinned = format!("{}@{}", root.name, exact);
        let hash = hash_for(&root.name, &pinned, infos)?;
        return Ok(Picked {
            name: root.name.clone(),
            version: exact.to_string(),
            hash,
        });
    }
    let got = info(&root.name, infos)?;
    let best = got
        .versions
        .iter()
        .filter_map(|v| Version::parse(&v.version).map(|p| (p, v)))
        .filter(|(p, _)| want.matches(p))
        .max_by(|(a, _), (b, _)| a.cmp(b));
    match best {
        Some((_, v)) => Ok(Picked {
            name: root.name.clone(),
            version: v.version.clone(),
            hash: v.hash.clone(),
        }),
        None => Err(format!(
            "{} has no version that fits {}; it has {}",
            root.name,
            root.constraint,
            got.versions
                .iter()
                .map(|v| v.version.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn info(name: &str, infos: &mut BTreeMap<String, PkgInfo>) -> Result<PkgInfo, String> {
    if let Some(got) = infos.get(name) {
        return Ok(got.clone());
    }
    let got = hub::info(name)?;
    infos.insert(name.to_string(), got.clone());
    Ok(got)
}

fn hash_for(
    name: &str,
    pinned: &str,
    infos: &mut BTreeMap<String, PkgInfo>,
) -> Result<String, String> {
    if let Some(hit) = infos.get(name).and_then(|got| {
        got.versions
            .iter()
            .find(|v| pinned == format!("{name}@{}", v.version))
    }) {
        return Ok(hit.hash.clone());
    }
    hub::hash_of(pinned)
}

/// split_pinned reads "name@version" apart.
pub fn split_pinned(pinned: &str) -> Option<(&str, &str)> {
    let at = pinned.rfind('@')?;
    Some((&pinned[..at], &pinned[at + 1..]))
}
