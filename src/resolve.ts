// The resolver.
//
// It walks the dependencies of a project and writes down what it found:
// every package, its exact version and its content hash, and the packages
// each one depends on.
//
// There is nothing to backtrack over. A Bend source names its
// dependencies exactly ("bend-ml-tensor-array@0.1.14.0"), the loader
// resolves a name and a version to a hash, and two versions of one
// package live in two directories and never meet. So the closure of a
// project is a set, not a choice: the resolver only has to walk it.

import * as hub from "./hub";
import { imports } from "./scan";
import * as lock from "./lock";
import { cmp, parse, parseConstraint, satisfies, show, type Version } from "./version";

export type Root = {
  name: string;
  constraint: string;
};

export class ResolveError extends Error {}

/** resolve walks the dependencies of a project and returns the closure. */
export async function resolve(roots: Root[]): Promise<lock.Lock> {
  const out = new Map<string, lock.Locked>();
  const infos = new Map<string, hub.PkgInfo>();
  const exacts = new Map<string, string>();
  const queue: Root[] = [...roots];

  while (queue.length > 0) {
    const root = queue.shift()!;
    const picked = await pick(root, infos, exacts);
    const pinned = picked.name + "@" + picked.version;
    if (out.has(pinned)) continue;

    const files = await hub.manifest(picked.hash);
    const deps: string[] = [];
    for (const f of files) {
      if (!f.path.endsWith(".bend")) continue;
      const text = await hub.file(picked.hash, f.path, f.sha);
      for (const p of imports(text)) {
        const dep = p.name + "@" + p.version;
        if (!deps.includes(dep)) deps.push(dep);
      }
    }
    out.set(pinned, {
      name: picked.name,
      version: picked.version,
      hash: picked.hash,
      source: "hub",
      dependencies: deps.sort(),
    });
    for (const dep of deps) {
      const at = dep.lastIndexOf("@");
      queue.push({ name: dep.slice(0, at), constraint: dep.slice(at + 1) });
    }
  }

  return { version: lock.LOCK_VERSION, package: [...out.values()] };
}
type Picked = {
  name: string;
  version: string;
  hash: string;
};

/**
 * pick chooses the version of a package a constraint asks for: the
 * highest one that satisfies it. An exact constraint reads the hash of
 * that version straight from the hub.
 */
async function pick(root: Root, infos: Map<string, hub.PkgInfo>,
  exacts: Map<string, string>): Promise<Picked> {
  const exact = parse(root.constraint);
  if (exact !== null && root.constraint.split(".").length === 4) {
    const pinned = root.name + "@" + show(exact);
    const hash = await hashFor(root.name, pinned, infos, exacts);
    return { name: root.name, version: show(exact), hash };
  }
  const got = await info(root.name, infos);
  const want = parseConstraint(root.constraint);
  const best = bestOf(got, want);
  if (best === null) {
    throw new ResolveError(root.name + " has no version that fits " + root.constraint
      + "; it has " + got.versions.map((v) => v.version).join(", "));
  }
  return { name: root.name, version: best.version, hash: best.hash };
}

/** bestOf picks the highest version of a package a constraint allows. */
export function bestOf(got: hub.PkgInfo, want: ReturnType<typeof parseConstraint>):
  hub.PkgVersion | null {
  let best: hub.PkgVersion | null = null;
  let bestV: Version | null = null;
  for (const v of got.versions) {
    const parsed = parse(v.version);
    if (parsed === null) continue;
    if (!satisfies(want, parsed)) continue;
    if (bestV === null || cmp(parsed, bestV) > 0) {
      best = v;
      bestV = parsed;
    }
  }
  return best;
}

async function info(name: string, infos: Map<string, hub.PkgInfo>): Promise<hub.PkgInfo> {
  const cached = infos.get(name);
  if (cached !== undefined) return cached;
  const got = await hub.info(name);
  infos.set(name, got);
  return got;
}

async function hashFor(name: string, pinned: string, infos: Map<string, hub.PkgInfo>,
  exacts: Map<string, string>): Promise<string> {
  const cached = exacts.get(pinned);
  if (cached !== undefined) return cached;
  const info = infos.get(name);
  if (info !== undefined) {
    const hit = info.versions.find((v) => pinned === name + "@" + v.version);
    if (hit !== undefined) {
      exacts.set(pinned, hit.hash);
      return hit.hash;
    }
  }
  const hash = await hub.hashOf(pinned);
  exacts.set(pinned, hash);
  return hash;
}
