//! Reading the imports out of a Bend source file.
//!
//! The loader reads the imports at the top of a file and stops at the
//! first line that is not one, so this does the same: a file's
//! dependencies are the pinned hub packages it names.

/// A pinned hub package: a name and a four-number version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pinned {
    pub name: String,
    pub version: String,
}

impl std::fmt::Display for Pinned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
    }
}

/// imports reads the pinned hub packages a source file depends on. It
/// returns them in the order they appear, without repeats.
pub fn imports(text: &str) -> Vec<Pinned> {
    let mut out: Vec<Pinned> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.starts_with("import") {
            break;
        }
        let rest = line[6..].trim_start();
        if rest.is_empty() {
            break;
        }
        // the path runs to the first space or the end of the line
        let path: String = rest
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '#')
            .collect();
        let (name, version) = match split_pinned(&path) {
            Some(got) => got,
            None => continue,
        };
        if !crate::hub::valid_name(name) {
            continue;
        }
        if crate::version::Version::parse(version).is_none() {
            continue;
        }
        let pinned = Pinned {
            name: name.to_string(),
            version: version.to_string(),
        };
        if !out.contains(&pinned) {
            out.push(pinned);
        }
    }
    out
}

/// split_pinned reads "name@version" apart.
fn split_pinned(path: &str) -> Option<(&str, &str)> {
    let at = path.rfind('@')?;
    let name = &path[..at];
    let version = &path[at + 1..];
    if name.is_empty() || version.is_empty() {
        return None;
    }
    // the version ends at the slash that starts the file's path
    let version = match version.find('/') {
        Some(slash) => &version[..slash],
        None => version,
    };
    Some((name, version))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(text: &str) -> Vec<String> {
        imports(text).iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn reads_a_pinned_hub_package() {
        let src = "import bend-ml-tensor-array@0.1.14.0/main.bend as TA\n";
        assert_eq!(names(src), vec!["bend-ml-tensor-array@0.1.14.0"]);
    }

    #[test]
    fn reads_several_in_order_without_repeats() {
        let src = [
            "import Base",
            "import bend-ml-tensor-array@0.1.14.0/main.bend as TA",
            "import bend-ml-tensor-fork@0.1.2.0/main.bend as TF",
            "import bend-ml-tensor-array@0.1.14.0/main.bend as TA2",
        ]
        .join("\n");
        assert_eq!(
            names(&src),
            vec![
                "bend-ml-tensor-array@0.1.14.0",
                "bend-ml-tensor-fork@0.1.2.0"
            ]
        );
    }

    #[test]
    fn ignores_a_relative_import() {
        assert!(imports("import ./src/shape/core.bend as Shape\n").is_empty());
    }

    #[test]
    fn ignores_base() {
        assert!(imports("import Base\n").is_empty());
    }

    #[test]
    fn stops_at_the_first_line_that_is_not_an_import() {
        let src = [
            "import bend-ml-tensor-array@0.1.14.0/main.bend as TA",
            "",
            "def main() -> U32:",
            "  0",
        ]
        .join("\n");
        assert_eq!(imports(&src).len(), 1);
    }

    #[test]
    fn reads_an_import_with_a_trailing_comment() {
        let src = "import bend-ml-tensor-array@0.1.14.0/main.bend as TA  # the tensors\n";
        assert_eq!(imports(src).len(), 1);
    }

    #[test]
    fn a_name_without_a_version_is_not_a_pinned_package() {
        assert!(imports("import bend-ml-tensor-array/main.bend as TA\n").is_empty());
    }
}
