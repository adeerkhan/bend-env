import { describe, expect, test } from "bun:test";

import { imports } from "../src/scan";

describe("imports", () => {
  test("reads a pinned hub package", () => {
    const src = "import bend-ml-tensor-array@0.1.14.0/main.bend as TA\n";
    expect(imports(src)).toEqual([
      { name: "bend-ml-tensor-array", version: "0.1.14.0" },
    ]);
  });

  test("reads several, in order, without repeats", () => {
    const src = [
      "import Base",
      "import bend-ml-tensor-array@0.1.14.0/main.bend as TA",
      "import bend-ml-tensor-fork@0.1.2.0/main.bend as TF",
      "import bend-ml-tensor-array@0.1.14.0/main.bend as TA2",
    ].join("\n");
    expect(imports(src).map((p) => p.name + "@" + p.version)).toEqual([
      "bend-ml-tensor-array@0.1.14.0",
      "bend-ml-tensor-fork@0.1.2.0",
    ]);
  });

  test("ignores a relative import", () => {
    const src = "import ./src/shape/core.bend as Shape\n";
    expect(imports(src)).toEqual([]);
  });

  test("ignores Base", () => {
    expect(imports("import Base\n")).toEqual([]);
  });

  test("stops at the first line that is not an import", () => {
    const src = [
      "import bend-ml-tensor-array@0.1.14.0/main.bend as TA",
      "",
      "def main() -> U32:",
      "  0",
    ].join("\n");
    expect(imports(src)).toHaveLength(1);
  });

  test("reads an import with a trailing comment", () => {
    const src = "import bend-ml-tensor-array@0.1.14.0/main.bend as TA  # the tensors\n";
    expect(imports(src)).toHaveLength(1);
  });

  test("a name without a version is not a pinned package", () => {
    expect(imports("import bend-ml-tensor-array/main.bend as TA\n")).toEqual([]);
  });
});
