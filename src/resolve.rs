//! The resolver.
//!
//! It walks the dependencies of a project and writes down what it found:
//! every package, its exact version and its content hash, the packages
//! each one depends on, and the files of each.
//!
//! There is nothing to backtrack over. A Bend source names its
//! dependencies exactly ("bend-ml-tensor-array@0.1.14.0"), the loader
//! resolves a name and version to a hash, and two versions of one
//! package live in two directories and never meet. So the closure of a
//! project is a set, not a choice: the resolver only has to walk it.
//!
//! The walk is level by level, and each level's questions go to the hub
//! at once. The hub answers in about a second, so a chain of four
//! dependencies is two seconds and not four.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use rayon::prelude::*;

use crate::cache;
use crate::hub::{self, PkgInfo};
use crate::lock::{LOCK_VERSION, Lock, Locked, LockedFile};
use crate::version::{Constraint, Version};

#[derive(Clone)]
pub struct Root {
    pub name: String,
    pub constraint: String,
}

/// What the walk is looking for: a package by a name and a constraint, or by
/// its hash.
enum Want {
    Named { name: String, constraint: String },
    Hash(String),
}

/// The pages of packages the walk has already read.
type Pages = Mutex<BTreeMap<String, PkgInfo>>;

/// A member of the workspace that lives on this machine: its name, its
/// version, and the directory it is in. Its content hash is the hash the
/// hub would give it, so a member that another member imports by name
/// resolves to the local build.
#[derive(Clone, Debug)]
pub struct Local {
    pub name: String,
    pub version: String,
    pub dir: PathBuf,
}

/// resolve walks the dependencies of a project and returns the closure.
pub fn resolve(roots: &[Root]) -> Result<Lock, String> {
    resolve_with(roots, &[])
}

/// resolve_with walks the dependencies of a project, the local members
/// among them, and returns the closure.
pub fn resolve_with(roots: &[Root], locals: &[Local]) -> Result<Lock, String> {
    let mut out: BTreeMap<String, Locked> = BTreeMap::new();
    let pages: Pages = Mutex::new(BTreeMap::new());
    let mut queue: Vec<Want> = roots
        .iter()
        .map(|r| Want::Named {
            name: r.name.clone(),
            constraint: r.constraint.clone(),
        })
        .collect();
    let mut asked: Vec<String> = Vec::new();

    // the local members first: a member that another member imports by
    // name must be in the closure under its own content hash
    for local in locals {
        let (files, entry) = crate::publish::pkg_files(&local.dir.join("main.bend"))?;
        let hash = crate::publish::hash_of(&files);
        let locked_files: Vec<LockedFile> = files
            .iter()
            .map(|(path, text)| LockedFile {
                sha: crate::hub::sha256(text),
                path: path.clone(),
            })
            .collect();
        let pinned = format!("{}@{}", local.name, local.version);
        if out.contains_key(&pinned) {
            continue;
        }
        out.insert(
            pinned.clone(),
            Locked {
                name: local.name.clone(),
                version: local.version.clone(),
                hash: hash.clone(),
                source: "local".to_string(),
                dependencies: Vec::new(),
                files: locked_files,
                constructors: Vec::new(),
            },
        );
        let _ = entry;

        // and the imports its own sources carry
        for text in files.values() {
            for p in crate::scan::imports(text) {
                let dep = p.to_string();
                if dep.starts_with("0x") {
                    queue.push(Want::Hash(dep));
                } else if let Some((n, v)) = split_pinned(&dep) {
                    queue.push(Want::Named {
                        name: n.to_string(),
                        constraint: v.to_string(),
                    });
                }
            }
        }
    }

    while !queue.is_empty() {
        // one level: every package the last level asked for, and each
        // question asked only once
        let level: Vec<Want> = queue
            .drain(..)
            .filter(|r| {
                let key = match r {
                    Want::Named { name, constraint } => format!("{name}@{constraint}"),
                    Want::Hash(hash) => hash.clone(),
                };
                if asked.contains(&key) {
                    return false;
                }
                asked.push(key);
                true
            })
            .collect();
        if level.is_empty() {
            break;
        }

        // the versions, asked at once. A package named by its hash needs
        // no question: the hash is the identity.
        let picks: Vec<Result<Picked, String>> = level
            .par_iter()
            .map(|root| match root {
                Want::Named { name, constraint } => pick(
                    &Root {
                        name: name.clone(),
                        constraint: constraint.clone(),
                    },
                    &pages,
                ),
                Want::Hash(hash) => Ok(Picked {
                    name: String::new(),
                    version: String::new(),
                    hash: hash.clone(),
                }),
            })
            .collect();
        let picks: Vec<Picked> = picks.into_iter().collect::<Result<Vec<_>, _>>()?;

        // the files of each, asked at once; the cache has them for good,
        // because a hash names the same files for ever
        let manifests: Vec<Result<Vec<hub::HubFile>, String>> =
            picks.par_iter().map(|p| cache::manifest(&p.hash)).collect();
        let manifests: Vec<Vec<hub::HubFile>> =
            manifests.into_iter().collect::<Result<Vec<_>, _>>()?;

        // the imports of each, read from its sources; the cache has them
        // for good too, for the same reason
        let deps_of: Vec<Result<Vec<String>, String>> =
            picks.par_iter().map(|p| cache::deps(&p.hash)).collect();
        let deps_of: Vec<Vec<String>> = deps_of.into_iter().collect::<Result<Vec<_>, _>>()?;

        // the types each declares, for the same reason
        let types_of: Vec<Result<Vec<String>, String>> = picks
            .par_iter()
            .map(|p| cache::constructors(&p.hash))
            .collect();
        let types_of: Vec<Vec<String>> = types_of.into_iter().collect::<Result<Vec<_>, _>>()?;

        for (i, picked) in picks.iter().enumerate() {
            let pinned = if picked.name.is_empty() {
                picked.hash.clone()
            } else {
                format!("{}@{}", picked.name, picked.version)
            };
            if out.contains_key(&pinned) {
                continue;
            }
            let mut deps: Vec<String> = deps_of[i].clone();
            deps.sort();
            out.insert(
                pinned.clone(),
                Locked {
                    name: picked.name.clone(),
                    version: picked.version.clone(),
                    hash: picked.hash.clone(),
                    source: "hub".to_string(),
                    dependencies: deps.clone(),
                    files: manifests[i]
                        .iter()
                        .map(|f| LockedFile {
                            sha: f.sha.clone(),
                            path: f.path.clone(),
                        })
                        .collect(),
                    constructors: types_of[i].clone(),
                },
            );
            for dep in deps {
                if dep.starts_with("0x") {
                    queue.push(Want::Hash(dep.clone()));
                } else if let Some(p) = split_pinned(&dep) {
                    queue.push(Want::Named {
                        name: p.0.to_string(),
                        constraint: p.1.to_string(),
                    });
                }
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
fn pick(root: &Root, pages: &Pages) -> Result<Picked, String> {
    let want = Constraint::parse(&root.constraint)?;
    if let Some(exact) = want.exact() {
        let pinned = format!("{}@{}", root.name, exact);
        let hash = hash_for(&root.name, &pinned, pages)?;
        return Ok(Picked {
            name: root.name.clone(),
            version: exact.to_string(),
            hash,
        });
    }
    let got = info(&root.name, pages)?;
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

fn info(name: &str, pages: &Pages) -> Result<PkgInfo, String> {
    if let Some(got) = pages.lock().unwrap().get(name) {
        return Ok(got.clone());
    }
    let got = cache::info(name)?;
    pages.lock().unwrap().insert(name.to_string(), got.clone());
    Ok(got)
}

fn hash_for(name: &str, pinned: &str, pages: &Pages) -> Result<String, String> {
    let hit = pages.lock().unwrap().get(name).and_then(|got| {
        got.versions
            .iter()
            .find(|v| pinned == format!("{name}@{}", v.version))
            .map(|v| v.hash.clone())
    });
    if let Some(hash) = hit {
        return Ok(hash);
    }
    cache::hash_of(pinned)
}

/// split_pinned reads "name@version" apart.
pub fn split_pinned(pinned: &str) -> Option<(&str, &str)> {
    let at = pinned.rfind('@')?;
    Some((&pinned[..at], &pinned[at + 1..]))
}
