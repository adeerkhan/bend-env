import { describe, expect, test } from "bun:test";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import * as lock from "../src/lock";
import * as manifest from "../src/manifest";

function tmp(): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), "bendenv-test-"));
}

describe("manifest", () => {
  test("reads a manifest", () => {
    const dir = tmp();
    const file = path.join(dir, "bend.toml");
    fs.writeFileSync(file, [
      "[project]",
      "name = \"demo\"",
      "version = \"0.1.0\"",
      "",
      "[dependencies]",
      "bend-ml-tensor-array = \"0.1.14.0\"",
      "",
    ].join("\n"));
    const m = manifest.read(file);
    expect(m.project.name).toBe("demo");
    expect(m.project.version).toBe("0.1.0");
    expect(m.dependencies["bend-ml-tensor-array"]).toBe("0.1.14.0");
  });

  test("round-trips a manifest", () => {
    const dir = tmp();
    const file = path.join(dir, "bend.toml");
    const m: manifest.Manifest = {
      project: { name: "demo", version: "0.1.0", description: "a demo" },
      dependencies: { "bend-ml-tensor-array": "^0.1.14", "bend-mathlib": "*" },
    };
    manifest.write(file, m);
    const back = manifest.read(file);
    expect(back.project.name).toBe("demo");
    expect(back.project.description).toBe("a demo");
    expect(back.dependencies).toEqual(m.dependencies);
  });

  test("refuses a manifest with no name", () => {
    const dir = tmp();
    const file = path.join(dir, "bend.toml");
    fs.writeFileSync(file, "[dependencies]\na = \"*\"\n");
    expect(() => manifest.read(file)).toThrow();
  });

  test("finds the manifest above the working directory", () => {
    const dir = tmp();
    const sub = path.join(dir, "a", "b");
    fs.mkdirSync(sub, { recursive: true });
    fs.writeFileSync(path.join(dir, "bend.toml"), "[project]\nname = \"x\"\n");
    expect(manifest.find(sub)).toBe(path.join(dir, "bend.toml"));
    expect(manifest.find(path.join(dir, ".."))).toBeNull();
  });
});

describe("lock", () => {
  test("round-trips a lockfile", () => {
    const dir = tmp();
    const file = path.join(dir, "bend.lock");
    const l: lock.Lock = {
      version: lock.LOCK_VERSION,
      package: [
        {
          name: "bend-ml-tensor-array",
          version: "0.1.14.0",
          hash: "0x0f6479f09ada7ae614e64e31e8f5a730",
          source: "hub",
          dependencies: [],
        },
        {
          name: "bend-ml-tensor-fork",
          version: "0.1.2.0",
          hash: "0x5734508f2a6198d9a87a28d4c58bfbae",
          source: "hub",
          dependencies: ["bend-ml-tensor-array@0.1.14.0"],
        },
      ],
    };
    lock.write(file, l);
    const back = lock.read(file);
    expect(back.package).toHaveLength(2);
    const fork = back.package.find((p) => p.name === "bend-ml-tensor-fork")!;
    expect(fork.dependencies).toEqual(["bend-ml-tensor-array@0.1.14.0"]);
    expect(lock.find(back, "bend-ml-tensor-fork@0.1.2.0")?.hash)
      .toBe("0x5734508f2a6198d9a87a28d4c58bfbae");
  });

  test("refuses a lockfile of another version", () => {
    const dir = tmp();
    const file = path.join(dir, "bend.lock");
    fs.writeFileSync(file, "version = 99\n");
    expect(() => lock.read(file)).toThrow();
  });
});
