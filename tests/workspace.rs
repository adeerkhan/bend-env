//! The workspace: a root manifest, its members, one lock, one
//! environment.

use bendenv::manifest::Manifest;
use bendenv::project;

fn tmp(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "bendenv-ws-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &std::path::Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, text).unwrap();
}

#[test]
fn a_workspace_names_its_members() {
    let dir = tmp("members");
    write(
        &dir.join("bend.toml"),
        "[project]\nname = \"my-ml\"\n\n[workspace]\nmembers = [\"packages/*\"]\n",
    );
    write(
        &dir.join("packages/metrics/bend.toml"),
        "[project]\nname = \"mlbend-metrics\"\n\n[dependencies]\n\
         bend-ml-nat-lemmas = \"0.1.1.0\"\n",
    );
    write(
        &dir.join("packages/preprocess/bend.toml"),
        "[project]\nname = \"mlbend-preprocess\"\n\n[dependencies]\n\
         bend-ml-tensor = \"0.1.0.0\"\n",
    );

    let p = project::open(&dir).expect("open");
    assert!(p.is_workspace());
    assert_eq!(p.members.len(), 2);
    assert_eq!(p.root, dir);

    // the roots are the union of every member's dependencies
    let mut roots: Vec<String> = p.roots().into_iter().map(|(n, _)| n).collect();
    roots.sort();
    assert_eq!(roots, vec!["bend-ml-nat-lemmas", "bend-ml-tensor"]);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_member_does_not_hide_the_workspace() {
    let dir = tmp("member-view");
    write(
        &dir.join("bend.toml"),
        "[project]\nname = \"my-ml\"\n\n[workspace]\nmembers = [\"packages/*\"]\n",
    );
    write(
        &dir.join("packages/metrics/bend.toml"),
        "[project]\nname = \"mlbend-metrics\"\n",
    );
    let member = dir.join("packages/metrics");

    // from inside the member, the project is still the workspace
    let p = project::open(&member).expect("open");
    assert!(p.is_workspace());
    assert_eq!(p.root, dir);
    assert_eq!(p.members.len(), 1);

    // and the nearest manifest is the member's own
    let (file, man) = p.nearest_manifest().expect("nearest");
    assert_eq!(man.project.name, "mlbend-metrics");
    assert!(file.ends_with("packages/metrics/bend.toml"));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_lone_package_is_not_a_workspace() {
    let dir = tmp("lone");
    write(&dir.join("bend.toml"), "[project]\nname = \"one\"\n");
    let p = project::open(&dir).expect("open");
    assert!(!p.is_workspace());
    assert_eq!(p.members.len(), 0);
    assert_eq!(p.root, dir);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_member_pattern_that_matches_nothing_is_an_error() {
    let dir = tmp("no-members");
    write(
        &dir.join("bend.toml"),
        "[project]\nname = \"my-ml\"\n\n[workspace]\nmembers = [\"packages/*\"]\n",
    );
    assert!(project::open(&dir).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn two_members_that_want_one_dependency_at_two_versions_are_reported() {
    let dir = tmp("skew");
    write(
        &dir.join("bend.toml"),
        "[project]\nname = \"my-ml\"\n\n[workspace]\nmembers = [\"packages/*\"]\n",
    );
    write(
        &dir.join("packages/a/bend.toml"),
        "[project]\nname = \"a\"\n\n[dependencies]\nbend-ml-tensor = \"0.1.0.0\"\n",
    );
    write(
        &dir.join("packages/b/bend.toml"),
        "[project]\nname = \"b\"\n\n[dependencies]\nbend-ml-tensor = \"0.1.2.0\"\n",
    );
    let p = project::open(&dir).expect("open");
    let skew = p.skew();
    assert_eq!(skew.len(), 1);
    assert_eq!(skew[0].0, "bend-ml-tensor");
    assert_eq!(skew[0].1, vec!["0.1.0.0", "0.1.2.0"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_manifest_round_trips_through_a_workspace() {
    let dir = tmp("round-trip");
    let mut man = Manifest::new("my-ml");
    man.as_workspace(vec!["packages/*".to_string()]);
    man.dependencies
        .insert("bend-ml-tensor".to_string(), "0.1.0.0".to_string());
    let file = dir.join("bend.toml");
    man.write(&file).expect("write");
    let back = Manifest::read(&file).expect("read");
    let ws = back.workspace.expect("workspace");
    assert_eq!(ws.members, vec!["packages/*"]);
    assert_eq!(back.dependencies.get("bend-ml-tensor").unwrap(), "0.1.0.0");
    std::fs::remove_dir_all(&dir).ok();
}
