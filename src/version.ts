// Versions and constraints.
//
// A BendHub package version is four numbers, major.minor.patch.build,
// the same shape the hub's NAMED regex accepts. A project version is
// free text; only the package versions are compared here.

export type Version = [number, number, number, number];

const V4 = /^(0|[1-9][0-9]*)(?:\.(0|[1-9][0-9]*))(?:\.(0|[1-9][0-9]*))?(?:\.(0|[1-9][0-9]*))?$/;

/** parse reads "1.2.3" or "1.2.3.4" into four numbers, padding with zeros. */
export function parse(text: string): Version | null {
  const m = V4.exec(text.trim());
  if (m === null) return null;
  return [Number(m[1]), Number(m[2]), Number(m[3] ?? 0), Number(m[4] ?? 0)];
}

/** show writes a version back in its four-number form. */
export function show(v: Version): string {
  return v.join(".");
}

/** cmp orders two versions: negative, zero or positive. */
export function cmp(a: Version, b: Version): number {
  for (let i = 0; i < 4; i++) {
    if (a[i]! !== b[i]!) return a[i]! < b[i]! ? -1 : 1;
  }
  return 0;
}

export type Term =
  | { op: "any" }
  | { op: "eq"; v: Version }
  | { op: "gt"; v: Version }
  | { op: "ge"; v: Version }
  | { op: "lt"; v: Version }
  | { op: "le"; v: Version };

export type Constraint = Term[];

export class ConstraintError extends Error {}

/**
 * parseConstraint reads a constraint string. It takes "any", an exact
 * version, a caret, a tilde, a comparator, or a comma-separated list of
 * comparators. A bare "1.2.3" is an exact version; "1.2" is the caret of
 * 1.2.0.0, the way a version prefix reads.
 */
export function parseConstraint(text: string): Constraint {
  const raw = text.trim();
  if (raw === "" || raw === "*" || raw.toLowerCase() === "latest") return [{ op: "any" }];
  const parts = raw.split(",").map((s) => s.trim()).filter((s) => s !== "");
  if (parts.length === 0) return [{ op: "any" }];
  return parts.flatMap(one);
}

function one(part: string): Term[] {
  if (part.startsWith("^")) return caret(part.slice(1));
  if (part.startsWith("~")) return tilde(part.slice(1));
  let op: "eq" | "gt" | "ge" | "lt" | "le" = "eq";
  let rest = part;
  if (rest.startsWith(">=")) { op = "ge"; rest = rest.slice(2); }
  else if (rest.startsWith("<=")) { op = "le"; rest = rest.slice(2); }
  else if (rest.startsWith(">")) { op = "gt"; rest = rest.slice(1); }
  else if (rest.startsWith("<")) { op = "lt"; rest = rest.slice(1); }
  else if (rest.startsWith("=")) { op = "eq"; rest = rest.slice(1); }
  const full = parse(rest);
  if (full === null) throw new ConstraintError("a version constraint: " + part);
  // "1.2" and "1.2.3" read as the caret of that version, not as an exact one.
  if (op === "eq" && rest.split(".").length < 4) return caret(rest);
  return [{ op, v: full }];
}

/**
 * caret reads "^1.2.3" as at least 1.2.3.0 and below 2.0.0.0, and
 * "^0.2.3" as below 0.3.0.0: the first number that is not zero is the one
 * that may move. "^0.0.3" stops at 0.0.4.0.
 */
function caret(text: string): Term[] {
  const parts = text.split(".");
  const base = parse(parts.join("."));
  if (base === null) throw new ConstraintError("a version constraint: ^" + text);
  const nums = parts.length;
  let upper: Version;
  if (base[0] !== 0) upper = [base[0] + 1, 0, 0, 0];
  else if (nums === 1) upper = [1, 0, 0, 0];
  else if (base[1] !== 0) upper = [0, base[1] + 1, 0, 0];
  else if (nums === 2) upper = [0, 1, 0, 0];
  else if (base[2] !== 0) upper = [0, 0, base[2] + 1, 0];
  else upper = [0, 0, 0, base[3] + 1];
  return [{ op: "ge", v: base }, { op: "lt", v: upper }];
}

/**
 * tilde reads "~1.2.3" as at least 1.2.3.0 and below 1.3.0.0: the minor
 * number moves, the major one does not. "~1" stops at 2.0.0.0.
 */
function tilde(text: string): Term[] {
  const parts = text.split(".");
  const base = parse(parts.join("."));
  if (base === null) throw new ConstraintError("a version constraint: ~" + text);
  const upper: Version = parts.length <= 1
    ? [base[0] + 1, 0, 0, 0]
    : [base[0], base[1] + 1, 0, 0];
  return [{ op: "ge", v: base }, { op: "lt", v: upper }];
}

/** satisfies tells whether a version meets a constraint. */
export function satisfies(c: Constraint, v: Version): boolean {
  return c.every((t) => termHolds(t, v));
}

function termHolds(t: Term, v: Version): boolean {
  switch (t.op) {
    case "any": return true;
    case "eq": return cmp(v, t.v) === 0;
    case "gt": return cmp(v, t.v) > 0;
    case "ge": return cmp(v, t.v) >= 0;
    case "lt": return cmp(v, t.v) < 0;
    case "le": return cmp(v, t.v) <= 0;
  }
}
