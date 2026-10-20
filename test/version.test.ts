import { describe, expect, test } from "bun:test";

import { cmp, parse, parseConstraint, satisfies, show } from "../src/version";

describe("parse", () => {
  test("reads four numbers", () => {
    expect(parse("1.2.3.4")).toEqual([1, 2, 3, 4]);
  });
  test("pads a shorter version with zeros", () => {
    expect(parse("1.2")).toEqual([1, 2, 0, 0]);
    expect(parse("1.2.3")).toEqual([1, 2, 3, 0]);
  });
  test("refuses a version that is not numbers", () => {
    expect(parse("1.2.x")).toBeNull();
    expect(parse("")).toBeNull();
    expect(parse("01.2.3.4")).toBeNull();
  });
  test("writes it back", () => {
    expect(show([0, 1, 14, 0])).toBe("0.1.14.0");
  });
});

describe("cmp", () => {
  test("orders by each number in turn", () => {
    expect(cmp([0, 1, 14, 0], [0, 1, 13, 9])).toBe(1);
    expect(cmp([0, 1, 14, 0], [0, 1, 14, 0])).toBe(0);
    expect(cmp([1, 0, 0, 0], [0, 9, 9, 9])).toBe(1);
  });
});

describe("constraints", () => {
  const v = (s: string) => parse(s)!;

  test("any takes every version", () => {
    for (const c of ["*", "latest", ""]) {
      expect(satisfies(parseConstraint(c), v("9.9.9.9"))).toBe(true);
    }
  });

  test("an exact version takes only itself", () => {
    const c = parseConstraint("0.1.14.0");
    expect(satisfies(c, v("0.1.14.0"))).toBe(true);
    expect(satisfies(c, v("0.1.14.1"))).toBe(false);
    expect(satisfies(c, v("0.1.13.0"))).toBe(false);
  });

  test("a caret moves the major, or the minor when the major is zero", () => {
    const c = parseConstraint("^1.2.3");
    expect(satisfies(c, v("1.2.3.0"))).toBe(true);
    expect(satisfies(c, v("1.9.9.9"))).toBe(true);
    expect(satisfies(c, v("2.0.0.0"))).toBe(false);
    expect(satisfies(c, v("1.2.2.9"))).toBe(false);
  });

  test("a caret under a zero major stops at the minor", () => {
    const c = parseConstraint("^0.1.14");
    expect(satisfies(c, v("0.1.14.0"))).toBe(true);
    expect(satisfies(c, v("0.1.99.0"))).toBe(true);
    expect(satisfies(c, v("0.2.0.0"))).toBe(false);
  });

  test("a caret under two zeros stops at the patch", () => {
    const c = parseConstraint("^0.0.3");
    expect(satisfies(c, v("0.0.3.0"))).toBe(true);
    expect(satisfies(c, v("0.0.4.0"))).toBe(false);
    expect(satisfies(c, v("0.1.0.0"))).toBe(false);
  });

  test("a tilde moves the minor only", () => {
    const c = parseConstraint("~1.2.3");
    expect(satisfies(c, v("1.2.3.0"))).toBe(true);
    expect(satisfies(c, v("1.2.99.0"))).toBe(true);
    expect(satisfies(c, v("1.3.0.0"))).toBe(false);
    expect(satisfies(c, v("2.0.0.0"))).toBe(false);
  });

  test("a bare short version reads as a caret", () => {
    const c = parseConstraint("1.2");
    expect(satisfies(c, v("1.2.0.0"))).toBe(true);
    expect(satisfies(c, v("1.9.0.0"))).toBe(true);
    expect(satisfies(c, v("2.0.0.0"))).toBe(false);
  });

  test("comparators stand alone", () => {
    expect(satisfies(parseConstraint(">=0.1.14.0"), v("0.1.14.0"))).toBe(true);
    expect(satisfies(parseConstraint(">0.1.14.0"), v("0.1.14.0"))).toBe(false);
    expect(satisfies(parseConstraint("<0.2.0.0"), v("0.1.99.0"))).toBe(true);
    expect(satisfies(parseConstraint("<=0.1.14.0"), v("0.1.14.0"))).toBe(true);
  });

  test("a list is a conjunction", () => {
    const c = parseConstraint(">=0.1.0.0, <0.2.0.0");
    expect(satisfies(c, v("0.1.14.0"))).toBe(true);
    expect(satisfies(c, v("0.2.0.0"))).toBe(false);
    expect(satisfies(c, v("0.0.9.0"))).toBe(false);
  });

  test("a version that is not a version is refused", () => {
    expect(() => parseConstraint("not-a-version")).toThrow();
  });
});
