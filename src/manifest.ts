// The project manifest: bend.toml.
//
// It names the project and lists what it depends on. A dependency is a
// name and a version constraint; the lockfile holds the exact versions
// the resolver chose.

import * as fs from "node:fs";
import * as path from "node:path";

export const MANIFEST_NAME = "bend.toml";

export type Manifest = {
  project: {
    name: string;
    version?: string;
    description?: string;
    /** the Bend version this project is written against */
    requires?: string;
  };
  dependencies: Record<string, string>;
  devDependencies?: Record<string, string>;
};

export class ManifestError extends Error {}

/** find walks up from a directory to the manifest at the root of the project. */
export function find(dir: string): string | null {
  let at = path.resolve(dir);
  for (;;) {
    const file = path.join(at, MANIFEST_NAME);
    if (fs.existsSync(file)) return file;
    const up = path.dirname(at);
    if (up === at) return null;
    at = up;
  }
}

/** read parses a manifest file. */
export function read(file: string): Manifest {
  let text: string;
  try {
    text = fs.readFileSync(file, "utf8");
  } catch {
    throw new ManifestError("no manifest at " + file);
  }
  let got: unknown;
  try {
    got = Bun.TOML.parse(text);
  } catch (err) {
    throw new ManifestError(file + " is not TOML: " + String(err));
  }
  const raw = (got ?? {}) as Record<string, unknown>;
  const project = (raw["project"] ?? {}) as Record<string, unknown>;
  const name = project["name"];
  if (typeof name !== "string" || name === "") {
    throw new ManifestError(file + " has no [project] name");
  }
  return {
    project: {
      name,
      version: str(project["version"]),
      description: str(project["description"]),
      requires: str(project["requires"]),
    },
    dependencies: deps(raw["dependencies"], file),
    devDependencies: deps(raw["devDependencies"], file),
  };
}

/** write puts a manifest back on disk, with its tables in a stable order. */
export function write(file: string, m: Manifest): void {
  const out: Record<string, unknown> = {
    project: {
      name: m.project.name,
      ...(m.project.version !== undefined ? { version: m.project.version } : {}),
      ...(m.project.description !== undefined ? { description: m.project.description } : {}),
      ...(m.project.requires !== undefined ? { requires: m.project.requires } : {}),
    },
  };
  out["dependencies"] = sorted(m.dependencies);
  if (m.devDependencies !== undefined && Object.keys(m.devDependencies).length > 0) {
    out["devDependencies"] = sorted(m.devDependencies);
  }
  fs.writeFileSync(file, Bun.TOML.stringify(out as never));
}

/** defaultFor writes a manifest for a new project. */
export function defaultFor(name: string): Manifest {
  return {
    project: { name, version: "0.1.0" },
    dependencies: {},
  };
}

function str(v: unknown): string | undefined {
  return typeof v === "string" ? v : undefined;
}

function deps(v: unknown, file: string): Record<string, string> {
  if (v === undefined) return {};
  if (typeof v !== "object" || v === null) {
    throw new ManifestError(file + " has a dependency table that is not a table");
  }
  const out: Record<string, string> = {};
  for (const [k, val] of Object.entries(v as Record<string, unknown>)) {
    if (typeof val !== "string") {
      throw new ManifestError(file + ": the constraint for " + k + " is not a string");
    }
    out[k] = val;
  }
  return out;
}

function sorted(d: Record<string, string>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const k of Object.keys(d).sort()) out[k] = d[k]!;
  return out;
}
