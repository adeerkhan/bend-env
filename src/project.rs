//! The project: a manifest, or a workspace of manifests.
//!
//! A workspace is what a monorepo of packages is: a root manifest that
//! names its members, one lockfile for all of them, and one environment
//! they all share. bend-ml and MLBend are both monorepos, so this is
//! the shape they want.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::manifest::{MANIFEST_NAME, Manifest};

/// the empty table, for a project that is not a workspace
static EMPTY: std::sync::LazyLock<BTreeMap<String, String>> =
    std::sync::LazyLock::new(BTreeMap::new);

/// A member of a workspace: a package with its own manifest.
#[derive(Clone, Debug)]
pub struct Member {
    pub dir: PathBuf,
    pub file: PathBuf,
    pub manifest: Manifest,
    /// the package's version, which is its name on the hub
    pub version: String,
}

impl Member {
    /// name is the package's name.
    pub fn name(&self) -> &str {
        &self.manifest.project.name
    }

    /// pinned is how the member is named: a name and a version.
    pub fn pinned(&self) -> String {
        format!("{}@{}", self.name(), self.version)
    }
}

#[derive(Clone, Debug)]
pub struct Project {
    /// the directory the search started from
    pub start: PathBuf,
    /// the root of the workspace, or the directory of a lone package
    pub root: PathBuf,
    pub file: PathBuf,
    pub manifest: Manifest,
    /// the members, when this is a workspace
    pub members: Vec<Member>,
}

impl Project {
    /// is_workspace says whether the project has members.
    pub fn is_workspace(&self) -> bool {
        !self.members.is_empty()
    }

    /// members_of returns the members, or the project itself when it is
    /// a lone package.
    pub fn packages(&self) -> Vec<(PathBuf, &Manifest)> {
        if self.members.is_empty() {
            return vec![(self.root.clone(), &self.manifest)];
        }
        let mut out: Vec<(PathBuf, &Manifest)> = self
            .members
            .iter()
            .map(|m| (m.dir.clone(), &m.manifest))
            .collect();
        out.push((self.root.clone(), &self.manifest));
        out
    }

    /// shared are the dependencies the workspace declares for its
    /// members: the workspace is the authority on their versions.
    pub fn shared(&self) -> &BTreeMap<String, String> {
        match &self.manifest.workspace {
            Some(ws) => &ws.dependencies,
            None => &EMPTY,
        }
    }

    /// roots are the dependencies to resolve: every package's, the
    /// direct ones first.
    ///
    /// A dependency the workspace also declares takes the workspace's
    /// constraint: one lock holds one version of each package, so the
    /// workspace has the last word.
    pub fn roots(&self) -> Vec<(String, String)> {
        let shared = self.shared();
        let mut out: Vec<(String, String)> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let take = |name: &String,
                    constraint: &String,
                    out: &mut Vec<(String, String)>,
                    seen: &mut Vec<String>| {
            if seen.contains(name) {
                return;
            }
            seen.push(name.clone());
            let want = shared
                .get(name)
                .cloned()
                .unwrap_or_else(|| constraint.clone());
            out.push((name.clone(), want));
        };
        for (_, man) in self.packages() {
            for (name, constraint) in &man.dependencies {
                take(name, constraint, &mut out, &mut seen);
            }
        }
        for (_, man) in self.packages() {
            for (name, constraint) in &man.dev_dependencies {
                take(name, constraint, &mut out, &mut seen);
            }
        }
        // the workspace's own shared dependencies are the project's too
        for (name, constraint) in shared {
            take(name, constraint, &mut out, &mut seen);
        }
        out
    }

    /// skew warns when two packages of the workspace want the same
    /// dependency at different versions.
    pub fn skew(&self) -> Vec<(String, Vec<String>)> {
        let mut by_name: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (_, man) in self.packages() {
            for (name, constraint) in man.dependencies.iter().chain(man.dev_dependencies.iter()) {
                by_name
                    .entry(name.clone())
                    .or_default()
                    .push(constraint.clone());
            }
        }
        by_name
            .into_iter()
            .filter(|(_, cs)| {
                cs.len() > 1 && cs.iter().collect::<std::collections::BTreeSet<_>>().len() > 1
            })
            .map(|(name, mut cs)| {
                cs.sort();
                cs.dedup();
                (name, cs)
            })
            .collect()
    }
}

/// find walks up from a directory to the manifest at the root of the
/// project.
pub fn find(dir: &Path) -> Option<PathBuf> {
    let mut at = dir.to_path_buf();
    loop {
        let file = at.join(MANIFEST_NAME);
        if file.is_file() {
            return Some(file);
        }
        match at.parent() {
            Some(up) if up != at => at = up.to_path_buf(),
            _ => return None,
        }
    }
}

/// chain reads every manifest from a directory up to the root, nearest
/// first.
fn chain(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut at = dir.to_path_buf();
    loop {
        let file = at.join(MANIFEST_NAME);
        if file.is_file() {
            out.push(file);
        }
        match at.parent() {
            Some(up) if up != at => at = up.to_path_buf(),
            _ => return out,
        }
    }
}

/// open reads the project a directory belongs to: a lone package, or the
/// workspace whose root manifest names it.
///
/// A member's own manifest does not hide the workspace: the search goes
/// up for a manifest that declares a workspace, and only then takes the
/// nearest one as a lone package.
pub fn open(dir: &Path) -> Result<Project, String> {
    let files = chain(dir);
    let nearest = files
        .first()
        .ok_or_else(|| format!("no {MANIFEST_NAME} here or above; run `bendenv init` first"))?;

    for file in &files {
        let manifest = Manifest::read(file)?;
        let Some(ws) = &manifest.workspace else {
            continue;
        };
        let root = file
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "the manifest has no directory".to_string())?;
        let mut members = Vec::new();
        for pattern in &ws.members {
            for dir in expand(&root, pattern)? {
                let mfile = dir.join(MANIFEST_NAME);
                if !mfile.is_file() {
                    return Err(format!(
                        "{} names {pattern} as a member, and {} has no {MANIFEST_NAME}",
                        file.display(),
                        dir.display()
                    ));
                }
                let man = Manifest::read(&mfile)?;
                let version = man.project.version.clone().unwrap_or_default();
                members.push(Member {
                    dir,
                    file: mfile,
                    manifest: man,
                    version,
                });
            }
        }
        return Ok(Project {
            start: dir.to_path_buf(),
            root,
            file: file.clone(),
            manifest,
            members,
        });
    }

    // no workspace above: the nearest manifest is a lone package
    let file = nearest.clone();
    let manifest = Manifest::read(&file)?;
    let root = file
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "the manifest has no directory".to_string())?;
    Ok(Project {
        start: dir.to_path_buf(),
        root,
        file,
        manifest,
        members: Vec::new(),
    })
}

/// expand reads the directories a member pattern names. A pattern with a
/// star matches the directories under its prefix that hold a manifest.
/// The star must be in the last segment of the pattern: "packages/*" and
/// "packages/ml-*", but not "packages/*/src".
fn expand(root: &Path, pattern: &str) -> Result<Vec<PathBuf>, String> {
    let full = root.join(pattern);
    let text = full.to_string_lossy().replace('\\', "/");
    let Some(star) = text.find('*') else {
        if full.is_dir() {
            return Ok(vec![full]);
        }
        return Err(format!("the member {pattern} is not a directory"));
    };
    let after = &text[star + 1..];
    if after.contains('/') {
        return Err(format!(
            "a member pattern with a star in its last segment: {pattern}"
        ));
    }
    // the directory that holds the segment the star is in, and the part
    // of that segment's name before the star
    let before = &text[..star];
    let (base, partial) = match before.rfind('/') {
        Some(i) => (PathBuf::from(&before[..i]), before[i + 1..].to_string()),
        None => (PathBuf::from("/"), before.to_string()),
    };
    let Ok(entries) = std::fs::read_dir(&base) else {
        return Err(format!(
            "cannot read {}: the member pattern {pattern} is not there",
            base.display()
        ));
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let matched = name.starts_with(&partial) && name.ends_with(after);
        if matched && path.join(MANIFEST_NAME).is_file() {
            out.push(path);
        }
    }
    out.sort();
    if out.is_empty() {
        return Err(format!("the member pattern {pattern} matched nothing"));
    }
    Ok(out)
}

impl Project {
    /// nearest_manifest returns the manifest of the package the working
    /// directory is in: a member of the workspace, or the root itself.
    pub fn nearest_manifest(&self) -> Result<(PathBuf, &Manifest), String> {
        let cwd = &self.start;
        let mut best: Option<(PathBuf, &Manifest, usize)> = None;
        for member in &self.members {
            if cwd.starts_with(&member.dir) {
                let depth = member.dir.components().count();
                if best.as_ref().map(|b| depth > b.2).unwrap_or(true) {
                    best = Some((member.file.clone(), &member.manifest, depth));
                }
            }
        }
        match best {
            Some((file, man, _)) => Ok((file, man)),
            None => Ok((self.file.clone(), &self.manifest)),
        }
    }
}
