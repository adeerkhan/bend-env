// The tests that talk to the hub. They need the network; the rest of the
// suite does not.

import { describe, expect, test } from "bun:test";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import * as env from "../src/env";
import * as hub from "../src/hub";
import { resolve } from "../src/resolve";

const TIMEOUT = 120_000;

function tmp(): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), "bendenv-hub-"));
}

describe("the hub", () => {
  test("answers a package's page with its versions", async () => {
    const got = await hub.info("bend-ml-tensor-array");
    expect(got.name).toBe("bend-ml-tensor-array");
    expect(got.versions.length).toBeGreaterThan(1);
    expect(got.latest.hash).toMatch(/^0x[0-9a-f]{32}$/);
    for (const v of got.versions) {
      expect(v.version).toMatch(/^\d+\.\d+\.\d+\.\d+$/);
      expect(v.hash).toMatch(/^0x[0-9a-f]{32}$/);
    }
  }, TIMEOUT);

  test("answers a pinned name with its hash", async () => {
    const hash = await hub.hashOf("bend-ml-tensor-array@0.1.14.0");
    expect(hash).toBe("0x0f6479f09ada7ae614e64e31e8f5a730");
  }, TIMEOUT);

  test("refuses a package it does not have", async () => {
    await expect(hub.info("bend-no-such-package-here")).rejects.toThrow();
  }, TIMEOUT);
});

describe("the resolver", () => {
  test("walks a package's dependencies", async () => {
    const l = await resolve([{ name: "bend-ml-tensor-fork", constraint: "*" }]);
    expect(l.package.length).toBe(2);
    const fork = l.package.find((p) => p.name === "bend-ml-tensor-fork")!;
    expect(fork).toBeDefined();
    expect(fork.dependencies).toEqual(["bend-ml-tensor-array@0.1.14.0"]);
    const array = l.package.find((p) => p.name === "bend-ml-tensor-array")!;
    expect(array.version).toBe("0.1.14.0");
    expect(array.hash).toBe("0x0f6479f09ada7ae614e64e31e8f5a730");
  }, TIMEOUT);

  test("takes the version a constraint names", async () => {
    const l = await resolve([{ name: "bend-ml-tensor-array", constraint: "0.1.12.0" }]);
    expect(l.package).toHaveLength(1);
    expect(l.package[0]!.version).toBe("0.1.12.0");
    expect(l.package[0]!.hash).toBe("0xf94fab1939abde90f6dd66413a7964d8");
  }, TIMEOUT);

  test("a caret takes the newest that fits", async () => {
    const l = await resolve([{ name: "bend-ml-tensor-array", constraint: "^0.1.13" }]);
    expect(l.package).toHaveLength(1);
    expect(l.package[0]!.version).toBe("0.1.14.0");
  }, TIMEOUT);

  test("a constraint nothing fits is an error", async () => {
    await expect(resolve([{ name: "bend-ml-tensor-array", constraint: "9.9.9.9" }]))
      .rejects.toThrow();
  }, TIMEOUT);
});

describe("the environment", () => {
  test("materializes a lock and reads back clean", async () => {
    const l = await resolve([{ name: "bend-ml-tensor-fork", constraint: "*" }]);
    const dir = tmp();
    const report = await env.sync(dir, l);
    expect(report.packages).toHaveLength(2);
    expect(report.fetched).toBeGreaterThan(0);

    // the shape the loader reads: a names directory and one directory per hash
    expect(fs.existsSync(path.join(dir, "names", "bend-ml-tensor-fork@0.1.2.0"))).toBe(true);
    expect(fs.readFileSync(path.join(dir, "names", "bend-ml-tensor-fork@0.1.2.0"), "utf8").trim())
      .toBe("0x5734508f2a6198d9a87a28d4c58bfbae");
    expect(fs.existsSync(path.join(dir, "0x5734508f2a6198d9a87a28d4c58bfbae", "main.bend")))
      .toBe(true);

    expect(await env.check(dir, l)).toEqual([]);
  }, TIMEOUT);

  test("a file that changed is found", async () => {
    const l = await resolve([{ name: "bend-ml-tensor-array", constraint: "0.1.14.0" }]);
    const dir = tmp();
    await env.sync(dir, l);
    const at = path.join(dir, "0x0f6479f09ada7ae614e64e31e8f5a730", "main.bend");
    fs.writeFileSync(at, "changed\n");
    const bad = await env.check(dir, l);
    expect(bad.length).toBeGreaterThan(0);
    expect(bad[0]).toContain("does not match");
  }, TIMEOUT);

  test("a second sync fetches nothing new", async () => {
    const l = await resolve([{ name: "bend-ml-tensor-array", constraint: "0.1.14.0" }]);
    const dir = tmp();
    await env.sync(dir, l);
    const again = await env.sync(dir, l);
    expect(again.fetched).toBe(0);
    expect(again.reused).toBeGreaterThan(0);
  }, TIMEOUT);
});
