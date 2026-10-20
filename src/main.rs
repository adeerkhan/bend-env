//! bendenv: a project and environment manager for Bend 2.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use bendenv::{cache, env, hub, lock, manifest, publish, resolve, version};
use lock::{LOCK_VERSION, Lock};
use manifest::Manifest;
use resolve::Root;

const USAGE: &str = "\
bendenv: a project and environment manager for Bend 2

  bendenv init [name]             write a manifest (bend.toml)
  bendenv add <name>[@constraint] add a dependency, lock and sync
  bendenv remove <name>           drop a dependency, lock and sync
  bendenv lock                    resolve the manifest into bend.lock
  bendenv sync                    materialize bend.lock into .bend/env
  bendenv list                    the packages the project resolves to
  bendenv tree                    the same, as a tree
  bendenv why <name>              why a package is in the lock
  bendenv outdated                the locked packages that have a newer version
  bendenv check                   verify the environment against the lock
  bendenv run -- <cmd>...         run a command with the environment
  bendenv publish <name>@<version> publish a package to the hub
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
        "lock" => lock_only(rest),
        "sync" => sync_only(rest),
        "list" => list(rest, false),
        "tree" => list(rest, true),
        "why" => why(rest),
        "outdated" => outdated(rest),
        "check" => check(rest),
        "run" => run(rest),
        "publish" => publish::publish(rest),
        "cache" => cache_cmd(rest),
        "purge" => purge(rest),
        "" | "-h" | "--help" | "help" => {
            print!("{USAGE}");
            Ok(())
        }
        other => Err(format!("an unknown command: {other}\n\n{USAGE}")),
    }
}

/// project finds the root of the project and its manifest.
struct Project {
    root: PathBuf,
    file: PathBuf,
    man: Manifest,
}

fn project() -> Result<Project, String> {
    let cwd =
        std::env::current_dir().map_err(|e| format!("cannot read the working directory: {e}"))?;
    let file = manifest::find(&cwd).ok_or_else(|| {
        format!(
            "no {} here or above; run `bendenv init` first",
            manifest::MANIFEST_NAME
        )
    })?;
    let man = Manifest::read(&file)?;
    let root = file
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "the manifest has no directory".to_string())?;
    Ok(Project { root, file, man })
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
    let name = args.first().cloned().unwrap_or_else(|| {
        cwd.file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    });
    Manifest::new(&name).write(&file)?;
    println!("wrote {} for {name}", manifest::MANIFEST_NAME);
    println!("next: bendenv add <package>");
    Ok(())
}

fn add(args: &[String]) -> Result<(), String> {
    let spec = args
        .first()
        .ok_or("a package to add: bendenv add <name>[@constraint]")?;
    let p = project()?;
    let (name, constraint) = match spec.rfind('@') {
        Some(at) if at > 0 => (spec[..at].to_string(), spec[at + 1..].to_string()),
        _ => (spec.clone(), "*".to_string()),
    };
    if !hub::valid_name(&name) {
        return Err(format!("a package name of a-z, 0-9 and -: {name}"));
    }
    version::Constraint::parse(&constraint)?;
    let mut man = p.man.clone();
    man.dependencies.insert(name.clone(), constraint.clone());
    man.write(&p.file)?;
    let l = resolve_roots(&man)?;
    write_lock(&p, &l)?;
    let report = env::sync(&env::env_path(&p.root), &l)?;
    println!(
        "added {name} {constraint} ({})",
        l.find(&format!(
            "{name}@{}",
            l.packages
                .iter()
                .find(|x| x.name == name)
                .map(|x| x.version.clone())
                .unwrap_or_default()
        ))
        .map(|x| x.version.clone())
        .unwrap_or_default()
    );
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
    Ok(())
}

fn remove(args: &[String]) -> Result<(), String> {
    let name = args
        .first()
        .ok_or("a package to remove: bendenv remove <name>")?;
    let p = project()?;
    if !p.man.dependencies.contains_key(name) {
        return Err(format!("{} does not depend on {name}", p.man.project.name));
    }
    let mut man = p.man.clone();
    man.dependencies.remove(name);
    man.write(&p.file)?;
    let l = resolve_roots(&man)?;
    write_lock(&p, &l)?;
    env::sync(&env::env_path(&p.root), &l)?;
    println!("removed {name}; locked {} packages", l.packages.len());
    Ok(())
}

fn lock_only(_args: &[String]) -> Result<(), String> {
    let p = project()?;
    let l = resolve_roots(&p.man)?;
    write_lock(&p, &l)?;
    println!("locked {} packages", l.packages.len());
    say_skew(&l);
    Ok(())
}

fn sync_only(_args: &[String]) -> Result<(), String> {
    let p = project()?;
    let l = Lock::read(&lock_file(&p.root))?;
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
    let p = project()?;
    let l = Lock::read(&lock_file(&p.root))?;
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
        return Ok(());
    }

    let mut by_pinned: BTreeMap<String, &lock::Locked> = BTreeMap::new();
    for pkg in &l.packages {
        by_pinned.insert(format!("{}@{}", pkg.name, pkg.version), pkg);
    }
    let mut shown: Vec<String> = Vec::new();
    let mut roots: Vec<String> = p.man.dependencies.keys().cloned().collect();
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
    let p = project()?;
    let l = Lock::read(&lock_file(&p.root))?;
    let mut by_pinned: BTreeMap<String, &lock::Locked> = BTreeMap::new();
    for pkg in &l.packages {
        by_pinned.insert(format!("{}@{}", pkg.name, pkg.version), pkg);
    }
    let mut roots: Vec<String> = p
        .man
        .dependencies
        .keys()
        .filter_map(|n| l.packages.iter().find(|x| &x.name == n))
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
    let p = project()?;
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
    let p = project()?;
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
    let p = project()?;
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
        _ => Err("bendenv cache clean | bendenv cache dir".to_string()),
    }
}

fn purge(_args: &[String]) -> Result<(), String> {
    let p = project()?;
    let e = env::env_path(&p.root);
    env::purge(&e)?;
    println!("removed {}", e.display());
    Ok(())
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

/// resolve_roots resolves the manifest's dependencies.
fn resolve_roots(man: &Manifest) -> Result<Lock, String> {
    let roots: Vec<Root> = man
        .roots()
        .into_iter()
        .map(|(name, constraint)| Root { name, constraint })
        .collect();
    if roots.is_empty() {
        return Ok(Lock::empty());
    }
    resolve::resolve(&roots)
}

fn write_lock(p: &Project, l: &Lock) -> Result<(), String> {
    l.write(&lock_file(&p.root))
}

/// the lock version this tool writes, for the tests
pub const LOCK_FORMAT: u32 = LOCK_VERSION;
