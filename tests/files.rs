//! The manifest, the lockfile and the environment, offline.

use bendenv::env;
use bendenv::lock::{LOCK_VERSION, Lock, Locked, LockedFile};
use bendenv::manifest::Manifest;

fn tmp(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "bendenv-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_manifest_round_trips() {
    let dir = tmp("manifest");
    let file = dir.join("bend.toml");
    let mut man = Manifest::new("demo");
    man.project.description = Some("a demo".to_string());
    man.dependencies
        .insert("bend-ml-tensor-array".to_string(), "^0.1.14".to_string());
    man.write(&file).expect("write");

    let back = Manifest::read(&file).expect("read");
    assert_eq!(back.project.name, "demo");
    assert_eq!(back.project.description.as_deref(), Some("a demo"));
    assert_eq!(
        back.dependencies.get("bend-ml-tensor-array").unwrap(),
        "^0.1.14"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_manifest_without_a_name_is_refused() {
    let dir = tmp("manifest-bad");
    let file = dir.join("bend.toml");
    std::fs::write(&file, "[dependencies]\na = \"*\"\n").unwrap();
    assert!(Manifest::read(&file).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_manifest_is_found_above_the_working_directory() {
    let dir = tmp("find");
    let sub = dir.join("a").join("b");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(dir.join("bend.toml"), "[project]\nname = \"x\"\n").unwrap();
    assert_eq!(bendenv::manifest::find(&sub), Some(dir.join("bend.toml")));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_lock_round_trips() {
    let dir = tmp("lock");
    let file = dir.join("bend.lock");
    let l = Lock {
        version: LOCK_VERSION,
        packages: vec![Locked {
            name: "bend-ml-tensor-array".to_string(),
            version: "0.1.14.0".to_string(),
            hash: "0x0f6479f09ada7ae614e64e31e8f5a730".to_string(),
            source: "hub".to_string(),
            dependencies: vec![],
            files: vec![LockedFile {
                sha: "a".repeat(64),
                path: "main.bend".to_string(),
            }],
            constructors: vec!["Mat".to_string()],
        }],
    };
    l.write(&file).expect("write");
    let back = Lock::read(&file).expect("read");
    assert_eq!(back.packages.len(), 1);
    assert_eq!(back.packages[0].files.len(), 1);
    assert_eq!(back.packages[0].files[0].path, "main.bend");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_lock_of_another_version_is_refused() {
    let dir = tmp("lock-bad");
    let file = dir.join("bend.lock");
    std::fs::write(&file, "version = 99\n").unwrap();
    assert!(Lock::read(&file).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_environment_is_verified_against_its_lock() {
    let dir = tmp("env");
    let e = dir.join(".bend").join("env");
    let l = Lock {
        version: LOCK_VERSION,
        packages: vec![Locked {
            name: "demo".to_string(),
            version: "0.1.0.0".to_string(),
            hash: "0x0123456789abcdef0123456789abcdef".to_string(),
            source: "hub".to_string(),
            dependencies: vec![],
            files: vec![LockedFile {
                sha: bendenv::hub::sha256("hello\n"),
                path: "main.bend".to_string(),
            }],
            constructors: vec!["Mat".to_string()],
        }],
    };
    // nothing installed yet
    let bad = env::check(&e, &l);
    assert_eq!(bad.len(), 1);
    assert!(bad[0].contains("not in the environment"));

    // install it by hand, the way sync would
    let pkg = e.join(&l.packages[0].hash);
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("main.bend"), "hello\n").unwrap();
    env::write_name(&e, "demo@0.1.0.0", &l.packages[0].hash).unwrap();
    assert!(env::check(&e, &l).is_empty());

    // a file that changed is found
    std::fs::write(pkg.join("main.bend"), "changed\n").unwrap();
    let bad = env::check(&e, &l);
    assert_eq!(bad.len(), 1);
    assert!(bad[0].contains("does not match"));

    std::fs::remove_dir_all(&dir).ok();
}
