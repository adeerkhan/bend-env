// The environment: a directory the loader reads as its package library.
//
// It is the shape bend already uses for its own cache, ~/.bend/lib: a
// "names" directory that maps a pinned name to a hash, and one
// directory per hash holding the files of that package. Setting
// BEND_LIB to the environment makes the loader read it and nothing else,
// so an environment is a package set, pinned and verified.

import * as fs from "node:fs";
import * as path from "node:path";

import * as hub from "./hub";
import * as lock from "./lock";

export const ENV_DIR = path.join(".bend", "env");

export class EnvError extends Error {}

export type SyncReport = {
  /** the packages that were written or already there */
  packages: string[];
  /** the files that were fetched */
  fetched: number;
  /** the files that were already there and matched */
  reused: number;
};

/** envPath is the environment of a project directory. */
export function envPath(root: string): string {
  return path.join(root, ENV_DIR);
}

/**
 * sync materializes a lockfile into an environment: every package's
 * files, verified against the SHA-256 the hub published, and the names
 * that point the loader at them.
 */
export async function sync(env: string, l: lock.Lock): Promise<SyncReport> {
  fs.mkdirSync(path.join(env, "names"), { recursive: true });
  const report: SyncReport = { packages: [], fetched: 0, reused: 0 };
  for (const pkg of l.package) {
    const dir = path.join(env, pkg.hash);
    fs.mkdirSync(dir, { recursive: true });
    for (const f of pkg.files) {
      const at = path.join(dir, f.path);
      if (fs.existsSync(at)) {
        const got = fs.readFileSync(at, "utf8");
        if (hub.sha256(got) === f.sha) {
          report.reused++;
          continue;
        }
      }
      const text = await hub.file(pkg.hash, f.path, f.sha);
      fs.mkdirSync(path.dirname(at), { recursive: true });
      fs.writeFileSync(at, text);
      report.fetched++;
    }
    writeName(env, pkg.name + "@" + pkg.version, pkg.hash);
    report.packages.push(pkg.name + "@" + pkg.version);
  }
  return report;
}

/** writeName points a pinned name at a hash, the way the loader reads it. */
export function writeName(env: string, pinned: string, hash: string): void {
  const at = path.join(env, "names", pinned);
  fs.mkdirSync(path.dirname(at), { recursive: true });
  fs.writeFileSync(at, hash + "\n");
}

/**
 * check verifies an environment against a lockfile: every package's
 * files are there and match the hash the hub published, and every name
 * points at the right hash. It returns the problems it found.
 */
export async function check(env: string, l: lock.Lock): Promise<string[]> {
  const bad: string[] = [];
  for (const pkg of l.package) {
    const dir = path.join(env, pkg.hash);
    if (!fs.existsSync(dir)) {
      bad.push(pkg.name + "@" + pkg.version + " is not in the environment");
      continue;
    }
    for (const f of pkg.files) {
      const at = path.join(dir, f.path);
      if (!fs.existsSync(at)) {
        bad.push(pkg.hash + "/" + f.path + " is missing");
        continue;
      }
      const got = fs.readFileSync(at, "utf8");
      if (hub.sha256(got) !== f.sha) {
        bad.push(pkg.hash + "/" + f.path + " does not match the hash the hub published");
      }
    }
    const nameAt = path.join(env, "names", pkg.name + "@" + pkg.version);
    if (!fs.existsSync(nameAt)) {
      bad.push(pkg.name + "@" + pkg.version + " has no name in the environment");
    } else if (fs.readFileSync(nameAt, "utf8").trim() !== pkg.hash) {
      bad.push(pkg.name + "@" + pkg.version + " points at another hash");
    }
  }
  return bad;
}

/** purge deletes an environment. */
export function purge(env: string): void {
  fs.rmSync(env, { recursive: true, force: true });
}

/**
 * present says which packages of a lockfile are already in the global
 * cache, so a resolver can use what is there without asking the hub.
 */
export function present(globalLib: string, l: lock.Lock): string[] {
  const out: string[] = [];
  for (const pkg of l.package) {
    if (fs.existsSync(path.join(globalLib, pkg.hash))) {
      out.push(pkg.name + "@" + pkg.version);
    }
  }
  return out;
}
