// The BendHub client.
//
// BendHub is the registry: it holds content-addressed packages, names a
// hash with a name and a four-number version, and serves the files of a
// package under that hash. Everything here verifies what it fetches
// against the SHA-256 the hub published, so a package that changed under
// us is an error and not a silent difference.

import { createHash } from "node:crypto";

export const HUB = process.env.BEND_HUB ?? "https://hub.bend-lang.com";

/** The name a package may carry: a-z, 0-9 and -, as the hub's NAMED says. */
export const NAME = /^[a-z][a-z0-9-]{0,63}$/;

/** A pinned reference: a name and a four-number version. */
export const PINNED = /^([a-z][a-z0-9-]{0,63})@((?:0|[1-9][0-9]*)(?:\.(?:0|[1-9][0-9]*)){3})$/;

export class HubError extends Error {}

export type PkgVersion = {
  version: string;
  hash: string;
  ts: number;
};

export type PkgInfo = {
  name: string;
  owner_login: string;
  ts: number;
  dependents: number;
  mentions: number;
  score: number;
  latest: {
    version: string;
    hash: string;
    ts: number;
    desc?: string;
    entry?: string;
    license?: { file: string; id: string };
  };
  versions: PkgVersion[];
};

export type HubFile = {
  /** the SHA-256 the hub published for the file */
  sha: string;
  /** the path of the file inside the package */
  path: string;
};

async function get(url: string): Promise<Response> {
  const res = await fetch(url, { signal: AbortSignal.timeout(60_000) }).catch(() => null);
  if (res === null) {
    throw new HubError("the hub could not be reached: " + url);
  }
  return res;
}

/** sha256 is the hex digest of a string or a buffer. */
export function sha256(data: string | Uint8Array): string {
  return createHash("sha256").update(data).digest("hex");
}

/**
 * info reads a package's page on the hub: its owner, its latest version
 * and every version it has published. A name the hub does not know is an
 * error; the message names the name, so a typo reads as a typo.
 */
export async function info(name: string): Promise<PkgInfo> {
  const res = await get(HUB + "/name/" + name);
  if (res.status === 404) {
    throw new HubError("no package named " + name + " on " + HUB);
  }
  if (!res.ok) {
    throw new HubError(HUB + " answered " + res.status + " for " + name);
  }
  const got = (await res.json().catch(() => null)) as PkgInfo | null;
  if (got === null || !Array.isArray(got.versions)) {
    throw new HubError(HUB + " did not answer with a package for " + name);
  }
  return got;
}

/** hashOf reads the hash a pinned name and version carry. */
export async function hashOf(pinned: string): Promise<string> {
  const res = await get(HUB + "/name/" + pinned);
  if (res.status === 404) {
    throw new HubError("no package " + pinned + " on " + HUB);
  }
  if (!res.ok) {
    throw new HubError(HUB + " answered " + res.status + " for " + pinned);
  }
  const got = (await res.text()).trim();
  if (!/^0x[0-9a-f]{32}$/.test(got)) {
    throw new HubError(HUB + " did not answer with a hash for " + pinned);
  }
  return got;
}

/**
 * manifest reads the files of a package: the SHA-256 of each file and its
 * path inside the package. The hub writes one "<sha> <path>" per line.
 */
export async function manifest(hash: string): Promise<HubFile[]> {
  const res = await get(HUB + "/" + hash + "/manifest");
  if (!res.ok) {
    throw new HubError(HUB + " answered " + res.status + " for the manifest of " + hash);
  }
  const text = await res.text();
  const out: HubFile[] = [];
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (trimmed === "") continue;
    const sp = trimmed.indexOf(" ");
    if (sp < 0) {
      throw new HubError("a manifest line of " + hash + " is not '<sha> <path>': " + trimmed);
    }
    out.push({ sha: trimmed.slice(0, sp), path: trimmed.slice(sp + 1) });
  }
  return out;
}

/**
 * file reads one file of a package and checks it against the SHA-256 the
 * manifest published. A file that does not match is refused.
 */
export async function file(hash: string, path: string, sha?: string): Promise<string> {
  const res = await get(HUB + "/" + hash + "/" + path);
  if (!res.ok) {
    throw new HubError(HUB + " answered " + res.status + " for " + hash + "/" + path);
  }
  const text = await res.text();
  if (sha !== undefined && sha256(text) !== sha) {
    throw new HubError(hash + "/" + path + " does not match the hash the hub published");
  }
  return text;
}
