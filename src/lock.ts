// The lockfile: bend.lock.
//
// It holds every package the project needs, the exact version and the
// content hash of each, and the packages each one depends on. The
// hashes are what make a run reproducible: a published version never
// moves, and the hash is checked against the bytes.

import * as fs from "node:fs";

export const LOCK_NAME = "bend.lock";

export type Locked = {
  name: string;
  version: string;
  hash: string;
  source: "hub";
  /** the pinned dependencies of this package, "name@version" each */
  dependencies: string[];
};

export type Lock = {
  version: number;
  package: Locked[];
};

export class LockError extends Error {}

export const LOCK_VERSION = 1;

/** read parses a lockfile. */
export function read(file: string): Lock {
  let text: string;
  try {
    text = fs.readFileSync(file, "utf8");
  } catch {
    throw new LockError("no lockfile at " + file);
  }
  let got: unknown;
  try {
    got = Bun.TOML.parse(text);
  } catch (err) {
    throw new LockError(file + " is not TOML: " + String(err));
  }
  const raw = (got ?? {}) as Record<string, unknown>;
  const version = raw["version"];
  if (version !== LOCK_VERSION) {
    throw new LockError(file + " has lock version " + String(version)
      + "; this tool writes version " + String(LOCK_VERSION));
  }
  const pkgs = raw["package"];
  if (pkgs !== undefined && !Array.isArray(pkgs)) {
    throw new LockError(file + " has a package table that is not an array of tables");
  }
  const out: Locked[] = [];
  for (const p of (pkgs ?? []) as Record<string, unknown>[]) {
    const name = p["name"];
    const ver = p["version"];
    const hash = p["hash"];
    if (typeof name !== "string" || typeof ver !== "string" || typeof hash !== "string") {
      throw new LockError(file + " has a package without a name, a version or a hash");
    }
    const deps = p["dependencies"];
    out.push({
      name,
      version: ver,
      hash,
      source: (p["source"] as "hub") ?? "hub",
      dependencies: Array.isArray(deps) ? deps.filter((d): d is string => typeof d === "string") : [],
    });
  }
  return { version: LOCK_VERSION, package: out };
}

/** write puts a lockfile back on disk, sorted so the diff is readable. */
export function write(file: string, lock: Lock): void {
  const pkgs = [...lock.package].sort((a, b) =>
    a.name === b.name ? cmpv(a.version, b.version) : a.name < b.name ? -1 : 1);
  const out = {
    version: LOCK_VERSION,
    package: pkgs.map((p) => ({
      name: p.name,
      version: p.version,
      hash: p.hash,
      source: p.source,
      dependencies: [...p.dependencies].sort(),
    })),
  };
  fs.writeFileSync(file, Bun.TOML.stringify(out as never));
}

/** empty is a lockfile with nothing in it. */
export function empty(): Lock {
  return { version: LOCK_VERSION, package: [] };
}

/** find returns the package of a name and version in a lock. */
export function find(lock: Lock, pinned: string): Locked | undefined {
  return lock.package.find((p) => p.name + "@" + p.version === pinned);
}

function cmpv(a: string, b: string): number {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < 4; i++) {
    const x = pa[i] ?? 0;
    const y = pb[i] ?? 0;
    if (x !== y) return x < y ? -1 : 1;
  }
  return 0;
}
