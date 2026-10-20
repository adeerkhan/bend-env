//! The project manifest: bend.toml.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const MANIFEST_NAME: &str = "bend.toml";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// the Bend version this project is written against
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires: Option<String>,
}

/// The workspace a root manifest names: the members it holds and the
/// dependencies they share.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Workspace {
    /// the member patterns, "packages/*" and the like
    #[serde(default)]
    pub members: Vec<String>,
    /// the dependencies the members share
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub project: Project,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
    #[serde(
        default,
        rename = "devDependencies",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub dev_dependencies: BTreeMap<String, String>,
    /// the members of a workspace, when this manifest is a workspace root
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<Workspace>,
}

impl Manifest {
    pub fn new(name: &str) -> Manifest {
        Manifest {
            project: Project {
                name: name.to_string(),
                version: Some("0.1.0".to_string()),
                description: None,
                requires: None,
            },
            dependencies: BTreeMap::new(),
            dev_dependencies: BTreeMap::new(),
            workspace: None,
        }
    }

    /// as_workspace turns a manifest into a workspace root with the
    /// members it names.
    pub fn as_workspace(&mut self, members: Vec<String>) {
        self.workspace = Some(Workspace {
            members,
            dependencies: BTreeMap::new(),
        });
    }

    /// read parses a manifest file.
    pub fn read(file: &Path) -> Result<Manifest, String> {
        let text =
            fs::read_to_string(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
        let got: Manifest = toml::from_str(&text)
            .map_err(|e| format!("{} is not a manifest: {e}", file.display()))?;
        if got.project.name.is_empty() {
            return Err(format!("{} has no [project] name", file.display()));
        }
        Ok(got)
    }

    /// write puts a manifest back on disk.
    pub fn write(&self, file: &Path) -> Result<(), String> {
        let text = toml::to_string_pretty(self)
            .map_err(|e| format!("cannot write {}: {e}", file.display()))?;
        fs::write(file, text).map_err(|e| format!("cannot write {}: {e}", file.display()))
    }

    /// roots are the dependencies to resolve, the direct ones first.
    pub fn roots(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self
            .dependencies
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        out.extend(
            self.dev_dependencies
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        out
    }
}

/// find walks up from a directory to the manifest at the root of the project.
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
