// bendenv: a project and environment manager for Bend 2.
//
//   bendenv init [name]            write a manifest
//   bendenv add <name>[@constraint] add a dependency, lock and sync
//   bendenv remove <name>          drop a dependency, lock and sync
//   bendenv lock                   resolve the manifest into bend.lock
//   bendenv sync                   materialize bend.lock into .bend/env
//   bendenv list                   the packages the project resolves to
//   bendenv tree                   the same, as a tree
//   bendenv check                  verify the environment against the lock
//   bendenv run -- <cmd>...        run a command with the environment
//   bendenv purge                  delete the environment

import * as fs from "node:fs";
import * as path from "node:path";

import * as env from "./env";
import * as hub from "./hub";
import * as lock from "./lock";
import * as manifest from "./manifest";
import { resolve, type Root } from "./resolve";
import { cmp, parse, parseConstraint, ConstraintError } from "./version";

const USAGE = `bendenv: a project and environment manager for Bend 2

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
  bendenv purge                   delete the environment
`;

function say(s: string): void {
  process.stdout.write(s + "\n");
}

function warn(s: string): void {
  process.stderr.write(s + "\n");
}

function fail(s: string): never {
  process.stderr.write("Error: " + s + "\n");
  process.exit(1);
}

/** project finds the root of the project and its manifest. */
function project(): { root: string; file: string; man: manifest.Manifest } {
  const cwd = process.cwd();
  const file = manifest.find(cwd);
  if (file === null) {
    fail("no " + manifest.MANIFEST_NAME + " here or above; run `bendenv init` first");
  }
  const root = path.dirname(file);
  return { root, file, man: manifest.read(file) };
}

function lockFile(root: string): string {
  return path.join(root, lock.LOCK_NAME);
}

async function main(argv: string[]): Promise<void> {
  const cmd = argv[0];
  const rest = argv.slice(1);
  switch (cmd) {
    case "init": return init(rest);
    case "add": return add(rest);
    case "remove": return remove(rest);
    case "lock": return lockOnly(rest);
    case "sync": return syncOnly(rest);
    case "list": return list(rest, false);
    case "tree": return list(rest, true);
    case "why": return why(rest);
    case "outdated": return outdated(rest);
    case "check": return check(rest);
    case "run": return run(rest);
    case "purge": return purge(rest);
    case undefined:
    case "-h":
    case "--help":
    case "help":
      say(USAGE);
      return;
    default:
      fail("an unknown command: " + cmd + "\n\n" + USAGE);
  }
}

// init writes a manifest for a new project.
function init(rest: string[]): void {
  const cwd = process.cwd();
  const file = path.join(cwd, manifest.MANIFEST_NAME);
  if (fs.existsSync(file)) {
    fail(cwd + " already has a " + manifest.MANIFEST_NAME);
  }
  const name = rest[0] ?? path.basename(cwd);
  const man = manifest.defaultFor(name);
  manifest.write(file, man);
  say("wrote " + manifest.MANIFEST_NAME + " for " + name);
  say("next: bendenv add <package>");
}

// add takes a dependency, locks and syncs.
async function add(rest: string[]): Promise<void> {
  const spec = rest[0];
  if (spec === undefined) fail("a package to add: bendenv add <name>[@constraint]");
  const p = project();
  const at = spec.lastIndexOf("@");
  const name = at > 0 ? spec.slice(0, at) : spec;
  const constraint = at > 0 ? spec.slice(at + 1) : "*";
  if (!hub.NAME.test(name)) {
    fail("a package name of a-z, 0-9 and -: " + name);
  }
  try {
    parseConstraint(constraint);
  } catch (err) {
    fail(String(err));
  }
  p.man.dependencies[name] = constraint;
  manifest.write(p.file, p.man);
  const l = await resolveRoots(p.man);
  lock.write(lockFile(p.root), l);
  const report = await env.sync(env.envPath(p.root), l);
  say("added " + name + " " + constraint + " ("
    + l.package.find((x) => x.name === name)?.version + ")");
  say("locked " + String(l.package.length) + " packages, "
    + String(report.fetched) + " files fetched");
  const exact = l.package.find((x) => x.name === name);
  if (exact !== undefined) {
    say("");
    say("import it with the exact version the lock holds:");
    say("");
    say("  import " + name + "@" + exact.version + "/<entry>.bend as <Name>");
  }
}

// remove drops a dependency, locks and syncs.
async function remove(rest: string[]): Promise<void> {
  const name = rest[0];
  if (name === undefined) fail("a package to remove: bendenv remove <name>");
  const p = project();
  if (p.man.dependencies[name] === undefined) {
    fail(p.man.project.name + " does not depend on " + name);
  }
  delete p.man.dependencies[name];
  manifest.write(p.file, p.man);
  const l = await resolveRoots(p.man);
  lock.write(lockFile(p.root), l);
  await env.sync(env.envPath(p.root), l);
  say("removed " + name + "; locked " + String(l.package.length) + " packages");
}

// lockOnly resolves the manifest and writes the lockfile.
async function lockOnly(_rest: string[]): Promise<void> {
  const p = project();
  const l = await resolveRoots(p.man);
  lock.write(lockFile(p.root), l);
  say("locked " + String(l.package.length) + " packages");
}

// syncOnly materializes the lockfile into the environment.
async function syncOnly(_rest: string[]): Promise<void> {
  const p = project();
  const l = lock.read(lockFile(p.root));
  const report = await env.sync(env.envPath(p.root), l);
  say("synced " + String(report.packages.length) + " packages, "
    + String(report.fetched) + " files fetched, "
    + String(report.reused) + " already there");
}

// list shows what the project resolves to.
async function list(_rest: string[], asTree: boolean): Promise<void> {
  const p = project();
  const l = lock.read(lockFile(p.root));
  if (l.package.length === 0) {
    say("no packages; run `bendenv add <name>`");
    return;
  }
  if (!asTree) {
    const width = Math.max(...l.package.map((x) => x.name.length));
    for (const pkg of [...l.package].sort((a, b) => a.name < b.name ? -1 : 1)) {
      say(pkg.name.padEnd(width) + "  " + pkg.version.padEnd(10) + " " + pkg.hash);
    }
    return;
  }
  const byPinned = new Map<string, lock.Locked>();
  for (const pkg of l.package) byPinned.set(pkg.name + "@" + pkg.version, pkg);
  const roots = Object.keys(p.man.dependencies).sort();
  const shown = new Set<string>();
  const walk = (pinned: string, depth: number): void => {
    const pkg = byPinned.get(pinned);
    if (pkg === undefined) {
      say("  ".repeat(depth) + "+- " + pinned + " (not locked)");
      return;
    }
    if (shown.has(pinned)) {
      say("  ".repeat(depth) + "+- " + pinned + " (already shown)");
      return;
    }
    shown.add(pinned);
    say("  ".repeat(depth) + (depth === 0 ? "" : "+- ") + pinned);
    for (const d of pkg.dependencies) walk(d, depth + 1);
  };
  for (const r of roots) {
    const pkg = l.package.find((x) => x.name === r);
    if (pkg === undefined) {
      say(r + " (not locked)");
      continue;
    }
    walk(pkg.name + "@" + pkg.version, 0);
  }
}

// why shows the path from a root dependency down to a package.
async function why(rest: string[]): Promise<void> {
  const name = rest[0];
  if (name === undefined) fail("a package: bendenv why <name>");
  const p = project();
  const l = lock.read(lockFile(p.root));
  const byPinned = new Map<string, lock.Locked>();
  for (const pkg of l.package) byPinned.set(pkg.name + "@" + pkg.version, pkg);
  const roots = Object.keys(p.man.dependencies)
    .map((n) => l.package.find((x) => x.name === n))
    .filter((x): x is lock.Locked => x !== undefined);

  const pathTo = (target: string): string[] | null => {
    const seen = new Set<string>();
    const walk = (pinned: string, trail: string[]): string[] | null => {
      const pkg = byPinned.get(pinned);
      if (pkg === undefined) return null;
      if (pkg.name === target) return [...trail, pinned];
      if (seen.has(pinned)) return null;
      seen.add(pinned);
      for (const d of pkg.dependencies) {
        const hit = walk(d, [...trail, pinned]);
        if (hit !== null) return hit;
      }
      return null;
    };
    for (const r of roots) {
      const hit = walk(r.name + "@" + r.version, []);
      if (hit !== null) return hit;
    }
    return null;
  };

  const hit = pathTo(name);
  if (hit === null) {
    say(name + " is not in the lock");
    return;
  }
  for (let i = 0; i < hit.length; i++) {
    say("  ".repeat(i) + (i === 0 ? "" : "+- ") + hit[i]);
  }
}

// outdated reports the locked packages that have a newer version on the hub.
async function outdated(_rest: string[]): Promise<void> {
  const p = project();
  const l = lock.read(lockFile(p.root));
  let any = false;
  for (const pkg of [...l.package].sort((a, b) => a.name < b.name ? -1 : 1)) {
    let got: hub.PkgInfo;
    try {
      got = await hub.info(pkg.name);
    } catch {
      continue;
    }
    const newest = got.versions
      .map((v) => ({ v, parsed: parse(v.version) }))
      .filter((x) => x.parsed !== null)
      .sort((a, b) => cmp(a.parsed!, b.parsed!))
      .pop();
    if (newest === undefined) continue;
    const now = parse(pkg.version)!;
    if (cmp(newest.parsed!, now) > 0) {
      any = true;
      say(pkg.name + " " + pkg.version + " -> " + newest.v.version);
    }
  }
  if (!any) say("every locked package is the newest on the hub");
}

// check verifies the environment against the lockfile.
async function check(_rest: string[]): Promise<void> {
  const p = project();
  const l = lock.read(lockFile(p.root));
  const bad = await env.check(env.envPath(p.root), l);
  if (bad.length === 0) {
    say("the environment matches the lock: " + String(l.package.length) + " packages");
    return;
  }
  for (const b of bad) warn("  " + b);
  fail(String(bad.length) + " problems in the environment");
}

// run runs a command with the environment as the package library.
async function run(rest: string[]): Promise<void> {
  const at = rest.indexOf("--");
  const cmd = at < 0 ? rest : rest.slice(at + 1);
  if (cmd.length === 0) fail("a command to run: bendenv run -- bend main.bend");
  const p = project();
  const e = env.envPath(p.root);
  if (!fs.existsSync(e)) {
    fail("no environment yet; run `bendenv sync`");
  }
  const proc = Bun.spawn(cmd, {
    cwd: p.root,
    env: { ...process.env, BEND_LIB: e },
    stdout: "inherit",
    stderr: "inherit",
    stdin: "inherit",
  });
  process.exit(await proc.exited);
}

// purge deletes the environment.
function purge(_rest: string[]): void {
  const p = project();
  env.purge(env.envPath(p.root));
  say("removed " + env.envPath(p.root));
}

/** resolveRoots resolves the manifest's dependencies, the ones it names first. */
async function resolveRoots(man: manifest.Manifest) {
  const roots: Root[] = [];
  for (const [name, constraint] of Object.entries(man.dependencies)) {
    roots.push({ name, constraint });
  }
  for (const [name, constraint] of Object.entries(man.devDependencies ?? {})) {
    roots.push({ name, constraint });
  }
  if (roots.length === 0) return lock.empty();
  return resolve(roots);
}

main(process.argv.slice(2)).catch((err: unknown) => {
  if (err instanceof ConstraintError) fail(err.message);
  if (err instanceof hub.HubError) fail(err.message);
  if (err instanceof Error) fail(err.message);
  fail(String(err));
});
