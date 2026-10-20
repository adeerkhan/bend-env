//! bendenv: a project and environment manager for Bend 2.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use bendenv::{cache, env, hub, lock, manifest, project, publish, resolve, version};
use lock::{LOCK_VERSION, Lock};
use manifest::Manifest;
use project::Project;
use resolve::Root;

const USAGE: &str = "\
bendenv: a project and environment manager for Bend 2

  bendenv init [name]             write a manifest (bend.toml)
  bendenv init --lib [name]       write a manifest for a package to publish
  bendenv init --workspace [name] write a manifest for a workspace of packages
  bendenv add <name>[@constraint] add a dependency, lock and sync
  bendenv add --dev <name>        add a development dependency
  bendenv remove <name>           drop a dependency, lock and sync
  bendenv lock                    resolve the manifest into bend.lock
  bendenv lock --check            say whether the lock is current
  bendenv sync                    materialize bend.lock into .bend/env
  bendenv sync --locked           sync without re-locking
  bendenv list                    the packages the project resolves to
  bendenv tree                    the same, as a tree
  bendenv why <name>              why a package is in the lock
  bendenv outdated                the locked packages that have a newer version
  bendenv check                   verify the environment against the lock
  bendenv run -- <cmd>...         run a command with the environment
  bendenv publish <name>@<version> publish a package to the hub
  bendenv hash <file.bend>         the hash a package would publish under
  bendenv cache clean             empty the global cache
  bendenv cache dir               where the global cache lives
  bendenv cache size              how much the global cache holds
  bendenv purge                   delete the environment
";

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if let Err(err) = dispatch(&argv) {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }
}

fn dispatch(argv: &[String]) -> Result<(), String> {
    let cmd = argv.first().map(String::as_str).unwrap_or("");
    let rest = &argv[1..];
    match cmd {
        "init" => init(rest),
        "add" => add(rest),
        "remove" => remove(rest),
        "lock" => lock_cmd(rest),
        "sync" => sync_only(rest),
        "list" => list(rest, false),
        "tree" => list(rest, true),
        "why" => why(rest),
        "outdated" => outdated(rest),
        "check" => check(rest),
        "run" => run(rest),
        "publish" => publish::publish(rest),
        "hash" => publish::hash_cmd(rest),
        "cache" => cache_cmd(rest),
        "purge" => purge(rest),
        "" | "-h" | "--help" | "help" => {
            print!("{USAGE}");
            Ok(())
        }
        other => Err(format!("an unknown command: {other}\n\n{USAGE}")),
    }
}

/// open reads the project the working directory belongs to.
fn open() -> Result<Project, String> {
    let cwd =
        std::env::current_dir().map_err(|e| format!("cannot read the working directory: {e}"))?;
    project::open(&cwd)
}

fn lock_file(root: &Path) -> PathBuf {
    root.join(lock::LOCK_NAME)
}

fn init(args: &[String]) -> Result<(), String> {
    let cwd =
        std::env::current_dir().map_err(|e| format!("cannot read the working directory: {e}"))?;
    let file = cwd.join(manifest::MANIFEST_NAME);
    if file.exists() {
        return Err(format!(
            "{} already has a {}",
            cwd.display(),
            manifest::MANIFEST_NAME
        ));
    }
    let lib = args.iter().any(|a| a == "--lib");
    let workspace = args.iter().any(|a| a == "--workspace");
    if lib && workspace {
        return Err("a package or a workspace, not both".to_string());
    }
    let name = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| {
            cwd.file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default()
        });

    let mut man = Manifest::new(&name);
    if lib {
        man.project.description = Some(format!("{name}, a Bend package"));
        write_license(&cwd)?;
    }
    if workspace {
        man.as_workspace(vec!["packages/*".to_string()]);
    }
    man.write(&file)?;
    println!("wrote {} for {name}", manifest::MANIFEST_NAME);
    if lib {
        println!("next: bendenv add <package>, then bendenv publish <name>@<version> <file>.bend");
    } else if workspace {
        println!("next: make packages/<name>/{}/", manifest::MANIFEST_NAME);
    } else {
        println!("next: bendenv add <package>");
    }
    Ok(())
}

/// write_license puts a LICENSE beside a package, the way the hub wants
/// one: it reads the license of the package it publishes.
fn write_license(dir: &Path) -> Result<(), String> {
    let at = dir.join("LICENSE");
    if at.exists() {
        return Ok(());
    }
    std::fs::write(
        &at,
        "MIT License\n\nCopyright (c) 2026\n\nPermission is hereby granted, free of charge, to any person obtaining a copy\nof this software and associated documentation files (the \"Software\"), to deal\nin the Software without restriction, including without limitation the rights\nto use, copy, modify, merge, publish, distribute, sublicense, and/or sell\ncopies of the Software, and to permit persons to whom the Software is\nfurnished to do so, subject to the following conditions:\n\nThe above copyright notice and this permission notice shall be included in all\ncopies or substantial portions of the Software.\n\nTHE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR\nIMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,\nFITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE\nAUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER\nLIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,\nOUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE\nSOFTWARE.\n",
    )
    .map_err(|e| format!("cannot write {}: {e}", at.display()))
}

/// add takes a dependency, locks and syncs.
fn add(args: &[String]) -> Result<(), String> {
    let dev = args.iter().any(|a| a == "--dev");
    let spec = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .ok_or("a package to add: bendenv add <name>[@constraint]")?;
    let p = open()?;
    let (file, man) = p.nearest_manifest()?;
    let (name, constraint) = match spec.rfind('@') {
        Some(at) if at > 0 => (spec[..at].to_string(), spec[at + 1..].to_string()),
        _ => (spec.clone(), "*".to_string()),
    };
    if !hub::valid_name(&name) {
        return Err(format!("a package name of a-z, 0-9 and -: {name}"));
    }
    version::Constraint::parse(&constraint)?;

    let mut man = man.clone();
    if dev {
        man.dev_dependencies
            .insert(name.clone(), constraint.clone());
    } else {
        man.dependencies.insert(name.clone(), constraint.clone());
    }
    man.write(&file)?;

    // read the project again: the manifest on disk is the new one
    let p = open()?;
    let l = resolve_project(&p)?;
    l.write(&lock_file(&p.root))?;
    let report = env::sync(&env::env_path(&p.root), &l)?;
    let got = l
        .packages
        .iter()
        .find(|x| x.name == name)
        .map(|x| x.version.clone())
        .unwrap_or_default();
    println!("added {name} {constraint} ({got})");
    println!(
        "locked {} packages, {} files fetched, {} installed",
        l.packages.len(),
        report.fetched,
        report.installed
    );
    if let Some(pkg) = l.packages.iter().find(|x| x.name == name) {
        println!();
        println!("import it with the exact version the lock holds:");
        println!();
        println!("  import {name}@{} /<entry>.bend as <Name>", pkg.version);
    }
    say_skew(&l);
    say_collisions(&l);
    say_workspace_skew(&p);
    Ok(())
}

/// remove drops a dependency, locks and syncs.
fn remove(args: &[String]) -> Result<(), String> {
    let name = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .ok_or("a package to remove: bendenv remove <name>")?;
    let p = open()?;
    let (file, man) = p.nearest_manifest()?;
    let mut man = man.clone();
    if man.dependencies.remove(name).is_none() && man.dev_dependencies.remove(name).is_none() {
        return Err(format!("{} does not depend on {name}", man.project.name));
    }
    man.write(&file)?;
    let p = open()?;
    let l = resolve_project(&p)?;
    l.write(&lock_file(&p.root))?;
    env::sync(&env::env_path(&p.root), &l)?;
    println!("removed {name}; locked {} packages", l.packages.len());
    Ok(())
}

/// lock_cmd resolves the manifest into the lockfile.
fn lock_cmd(args: &[String]) -> Result<(), String> {
    let p = open()?;
    let want = resolve_project(&p)?;
    let at = lock_file(&p.root);
    if args.iter().any(|a| a == "--check") {
        let have = Lock::read(&at)?;
        let same = have.packages.len() == want.packages.len()
            && have.packages.iter().all(|x| {
                want.packages
                    .iter()
                    .any(|y| y.name == x.name && y.version == x.version && y.hash == x.hash)
            });
        if same {
            println!("the lock is current");
            return Ok(());
        }
        return Err("the lock is not current: run `bendenv lock`".to_string());
    }
    want.write(&at)?;
    println!("locked {} packages", want.packages.len());
    say_skew(&want);
    say_collisions(&want);
    say_workspace_skew(&p);
    Ok(())
}

fn sync_only(args: &[String]) -> Result<(), String> {
    let p = open()?;
    let at = lock_file(&p.root);
    // --locked uses the lock as it is: no resolving, no network
    let l = if args.iter().any(|a| a == "--locked") {
        if !at.is_file() {
            return Err("no bend.lock: run `bendenv lock`".to_string());
        }
        Lock::read(&at)?
    } else {
        let l = resolve_project(&p)?;
        l.write(&at)?;
        l
    };
    let report = env::sync(&env::env_path(&p.root), &l)?;
    println!(
        "synced {} packages, {} files fetched, {} linked, {} already there",
        report.packages.len(),
        report.fetched,
        report.linked,
        report.reused
    );
    Ok(())
}

fn list(_args: &[String], as_tree: bool) -> Result<(), String> {
    let p = open()?;
    let l = read_lock_lenient(&p.root);
    if p.is_workspace() {
        println!("workspace {}", p.manifest.project.name);
        for m in &p.members {
            println!("  member {} at {}", m.name(), m.dir.display());
        }
        println!();
    }
    if l.packages.is_empty() {
        println!("no packages; run `bendenv add <name>`");
        return Ok(());
    }
    if !as_tree {
        let width = l.packages.iter().map(|x| x.name.len()).max().unwrap_or(0);
        let mut packages = l.packages.clone();
        packages.sort_by(|a, b| a.name.cmp(&b.name));
        for pkg in packages {
            println!(
                "{:<width$}  {:<10} {}",
                pkg.name,
                pkg.version,
                pkg.hash,
                width = width
            );
        }
        say_skew(&l);
        say_collisions(&l);
        say_workspace_skew(&p);
        return Ok(());
    }

    let mut by_pinned: BTreeMap<String, &lock::Locked> = BTreeMap::new();
    for pkg in &l.packages {
        by_pinned.insert(format!("{}@{}", pkg.name, pkg.version), pkg);
    }
    let mut shown: Vec<String> = Vec::new();
    let mut roots: Vec<String> = p.roots().into_iter().map(|(n, _)| n).collect();
    roots.sort();
    for r in roots {
        let Some(pkg) = l.packages.iter().find(|x| x.name == r) else {
            println!("{r} (not locked)");
            continue;
        };
        walk(
            &by_pinned,
            &format!("{}@{}", pkg.name, pkg.version),
            0,
            &mut shown,
        );
    }
    Ok(())
}

fn walk(
    by_pinned: &BTreeMap<String, &lock::Locked>,
    pinned: &str,
    depth: usize,
    shown: &mut Vec<String>,
) {
    let Some(pkg) = by_pinned.get(pinned) else {
        println!("{}+- {pinned} (not locked)", "  ".repeat(depth));
        return;
    };
    if shown.iter().any(|s| s == pinned) {
        println!("{}+- {pinned} (already shown)", "  ".repeat(depth));
        return;
    }
    shown.push(pinned.to_string());
    println!(
        "{}{}{pinned}",
        "  ".repeat(depth),
        if depth == 0 { "" } else { "+- " }
    );
    for d in &pkg.dependencies {
        walk(by_pinned, d, depth + 1, shown);
    }
}

fn why(args: &[String]) -> Result<(), String> {
    let name = args.first().ok_or("a package: bendenv why <name>")?;
    let p = open()?;
    let l = Lock::read(&lock_file(&p.root))?;
    let mut by_pinned: BTreeMap<String, &lock::Locked> = BTreeMap::new();
    for pkg in &l.packages {
        by_pinned.insert(format!("{}@{}", pkg.name, pkg.version), pkg);
    }
    let mut roots: Vec<String> = p
        .roots()
        .into_iter()
        .filter_map(|(n, _)| l.packages.iter().find(|x| x.name == n))
        .map(|x| format!("{}@{}", x.name, x.version))
        .collect();
    roots.sort();
    for r in roots {
        if let Some(trail) = path_to(&by_pinned, &r, name, &mut Vec::new()) {
            for (i, step) in trail.iter().enumerate() {
                println!(
                    "{}{}{step}",
                    "  ".repeat(i),
                    if i == 0 { "" } else { "+- " }
                );
            }
            return Ok(());
        }
    }
    println!("{name} is not in the lock");
    Ok(())
}

fn path_to(
    by_pinned: &BTreeMap<String, &lock::Locked>,
    pinned: &str,
    target: &str,
    seen: &mut Vec<String>,
) -> Option<Vec<String>> {
    let pkg = by_pinned.get(pinned)?;
    if pkg.name == target {
        return Some(vec![pinned.to_string()]);
    }
    if seen.iter().any(|s| s == pinned) {
        return None;
    }
    seen.push(pinned.to_string());
    for d in &pkg.dependencies {
        if let Some(mut trail) = path_to(by_pinned, d, target, seen) {
            trail.insert(0, pinned.to_string());
            return Some(trail);
        }
    }
    None
}

fn outdated(_args: &[String]) -> Result<(), String> {
    let p = open()?;
    let l = Lock::read(&lock_file(&p.root))?;
    let mut any = false;
    let mut packages = l.packages.clone();
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    for pkg in packages {
        let Ok(got) = hub::info(&pkg.name) else {
            continue;
        };
        let Some(newest) = got
            .versions
            .iter()
            .filter_map(|v| version::Version::parse(&v.version).map(|p| (p, v)))
            .max_by(|(a, _), (b, _)| a.cmp(b))
        else {
            continue;
        };
        let now = version::Version::parse(&pkg.version).unwrap_or(version::Version(0, 0, 0, 0));
        if newest.0 > now {
            any = true;
            println!("{} {} -> {}", pkg.name, pkg.version, newest.1.version);
        }
    }
    if !any {
        println!("every locked package is the newest on the hub");
    }
    Ok(())
}

fn check(_args: &[String]) -> Result<(), String> {
    let p = open()?;
    let l = Lock::read(&lock_file(&p.root))?;
    let bad = env::check(&env::env_path(&p.root), &l);
    if bad.is_empty() {
        println!(
            "the environment matches the lock: {} packages",
            l.packages.len()
        );
        return Ok(());
    }
    for b in &bad {
        eprintln!("  {b}");
    }
    Err(format!("{} problems in the environment", bad.len()))
}

fn run(args: &[String]) -> Result<(), String> {
    let at = args.iter().position(|a| a == "--");
    let cmd: Vec<String> = match at {
        Some(i) => args[i + 1..].to_vec(),
        None => args.to_vec(),
    };
    if cmd.is_empty() {
        return Err("a command to run: bendenv run -- bend main.bend".to_string());
    }
    let p = open()?;
    let e = env::env_path(&p.root);
    if !e.is_dir() {
        return Err("no environment yet; run `bendenv sync`".to_string());
    }
    let status = Command::new(&cmd[0])
        .args(&cmd[1..])
        .current_dir(&p.root)
        .env("BEND_LIB", &e)
        .status()
        .map_err(|e| format!("cannot run {}: {e}", cmd[0]))?;
    std::process::exit(status.code().unwrap_or(1));
}

fn cache_cmd(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("clean") => {
            let n = cache::clean()?;
            println!("removed {n} files from the cache");
            Ok(())
        }
        Some("dir") => {
            println!("{}", cache::cache_dir().display());
            Ok(())
        }
        Some("size") => {
            println!(
                "{} bytes in {}",
                cache::size(),
                cache::cache_dir().display()
            );
            Ok(())
        }
        _ => Err("bendenv cache clean | bendenv cache dir | bendenv cache size".to_string()),
    }
}

fn purge(_args: &[String]) -> Result<(), String> {
    let p = open()?;
    let e = env::env_path(&p.root);
    env::purge(&e)?;
    println!("removed {}", e.display());
    Ok(())
}

/// read_lock_lenient reads the lockfile, and treats a missing one as an
/// empty lock: a project that has not been locked yet still has members
/// to show.
fn read_lock_lenient(root: &Path) -> Lock {
    Lock::read(&lock_file(root)).unwrap_or_else(|_| Lock::empty())
}

/// say_collisions prints the type names two packages of the closure
/// both declare. A Bend type belongs to the package that declares it,
/// so two packages that declare `Mat` are two different `Mat`, and a
/// program that mixes them does not compile.
///
/// Base is in the report too: it is not on the hub and it moves, so a
/// constructor it gains can break a package that was published before.
fn say_collisions(l: &Lock) {
    let mut bad = l.collisions();
    let base = cache::base_constructors();
    if !base.is_empty() {
        for (name, packages) in bad.iter_mut() {
            if base.contains(name) {
                packages.push("bend2/base.bend".to_string());
                packages.sort();
            }
        }
    }
    if bad.is_empty() {
        return;
    }
    eprintln!();
    eprintln!("two packages of the closure declare a type with the same name.");
    eprintln!("A Bend type belongs to the package that declares it, so they are");
    eprintln!("two different types: a program that passes one where the other");
    eprintln!("is expected does not compile. The names that matter are the ones");
    eprintln!("both stacks use; the rest only collide if they are mixed:");
    for (name, packages) in bad {
        eprintln!("  {name}: {}", packages.join(", "));
    }
}

/// say_skew prints the packages the closure holds at two versions.
fn say_skew(l: &Lock) {
    let bad = l.skew();
    if bad.is_empty() {
        return;
    }
    eprintln!();
    eprintln!("two versions of the same package are in the lock. They are two");
    eprintln!("different types, and the checker refuses to mix them:");
    for (name, versions) in bad {
        eprintln!("  {name}: {}", versions.join(", "));
    }
}

/// say_workspace_skew prints the dependencies two packages of a
/// workspace want at different versions.
fn say_workspace_skew(p: &Project) {
    let bad = p.skew();
    if bad.is_empty() {
        return;
    }
    eprintln!();
    eprintln!("two packages of the workspace want the same dependency at");
    eprintln!("different versions. One lock holds one version of each:");
    for (name, versions) in bad {
        eprintln!("  {name}: {}", versions.join(", "));
    }
}

/// resolve_project resolves the dependencies of every package of the
/// project: the workspace's, or the lone package's.
fn resolve_project(p: &Project) -> Result<Lock, String> {
    let roots: Vec<Root> = p
        .roots()
        .into_iter()
        .map(|(name, constraint)| Root { name, constraint })
        .collect();
    if roots.is_empty() {
        return Ok(Lock::empty());
    }
    resolve::resolve(&roots)
}

/// the lock version this tool writes, for the tests
pub const LOCK_FORMAT: u32 = LOCK_VERSION;
