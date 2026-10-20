//! The tests that talk to the hub. They need the network; the rest of
//! the suite does not.

use bendenv::hub;
use bendenv::lock::Lock;
use bendenv::resolve::{Root, resolve};
use std::collections::BTreeMap;

fn roots(pairs: &[(&str, &str)]) -> Vec<Root> {
    pairs
        .iter()
        .map(|(name, constraint)| Root {
            name: name.to_string(),
            constraint: constraint.to_string(),
        })
        .collect()
}

#[test]
fn the_hub_answers_a_package_page() {
    let got = hub::info("bend-ml-tensor-array").expect("the hub");
    assert_eq!(got.name, "bend-ml-tensor-array");
    assert!(got.versions.len() > 1);
    for v in &got.versions {
        assert!(hub::valid_hash(&v.hash), "{} is not a hash", v.hash);
        assert_eq!(v.version.split('.').count(), 4);
    }
}

#[test]
fn the_hub_answers_a_pinned_name_with_its_hash() {
    let hash = hub::hash_of("bend-ml-tensor-array@0.1.14.0").expect("the hub");
    assert_eq!(hash, "0x0f6479f09ada7ae614e64e31e8f5a730");
}

#[test]
fn the_hub_refuses_a_package_it_does_not_have() {
    assert!(hub::info("bend-no-such-package-here").is_err());
}

#[test]
fn the_resolver_walks_a_package_dependencies() {
    let l = resolve(&roots(&[("bend-ml-tensor-fork", "*")])).expect("resolve");
    assert_eq!(l.packages.len(), 2);
    let fork = l
        .packages
        .iter()
        .find(|p| p.name == "bend-ml-tensor-fork")
        .unwrap();
    assert_eq!(fork.dependencies, vec!["bend-ml-tensor-array@0.1.14.0"]);
    let array = l
        .packages
        .iter()
        .find(|p| p.name == "bend-ml-tensor-array")
        .unwrap();
    assert_eq!(array.version, "0.1.14.0");
    assert_eq!(array.hash, "0x0f6479f09ada7ae614e64e31e8f5a730");
    assert!(!array.files.is_empty());
}

#[test]
fn the_resolver_takes_the_version_a_constraint_names() {
    let l = resolve(&roots(&[("bend-ml-tensor-array", "0.1.12.0")])).expect("resolve");
    assert_eq!(l.packages.len(), 1);
    assert_eq!(l.packages[0].version, "0.1.12.0");
    assert_eq!(l.packages[0].hash, "0xf94fab1939abde90f6dd66413a7964d8");
}

#[test]
fn the_resolver_takes_the_newest_that_fits_a_caret() {
    let l = resolve(&roots(&[("bend-ml-tensor-array", "^0.1.13")])).expect("resolve");
    assert_eq!(l.packages.len(), 1);
    assert_eq!(l.packages[0].version, "0.1.14.0");
}

#[test]
fn a_constraint_nothing_fits_is_an_error() {
    assert!(resolve(&roots(&[("bend-ml-tensor-array", "9.9.9.9")])).is_err());
}

#[test]
fn the_closure_can_hold_one_package_at_two_versions() {
    let l = resolve(&roots(&[
        ("bend-ml-tensor", "0.1.2.0"),
        ("bend-ml-autograd", "0.1.1.0"),
    ]))
    .expect("resolve");
    let mut by_name: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for pkg in &l.packages {
        by_name
            .entry(pkg.name.clone())
            .or_default()
            .push(pkg.version.clone());
    }
    let mut versions = by_name.get("bend-ml-tensor").unwrap().clone();
    versions.sort();
    assert_eq!(versions, vec!["0.1.0.0", "0.1.2.0"]);
    assert_eq!(by_name.get("bend-ml-autograd").unwrap().len(), 1);

    // and the lock says so
    let skew = l.skew();
    assert_eq!(skew.len(), 1);
    assert_eq!(skew[0].0, "bend-ml-tensor");
    assert_eq!(skew[0].1, vec!["0.1.0.0", "0.1.2.0"]);
}

#[test]
fn the_lock_round_trips_through_a_file() {
    let dir = std::env::temp_dir().join(format!("bendenv-lock-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("bend.lock");
    let l = resolve(&roots(&[("bend-ml-tensor-fork", "*")])).expect("resolve");
    l.write(&file).expect("write");
    let back = Lock::read(&file).expect("read");
    assert_eq!(back.packages.len(), l.packages.len());
    assert_eq!(back.packages[0].files.len(), l.packages[0].files.len());
    std::fs::remove_dir_all(&dir).ok();
}
