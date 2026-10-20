// Reading the imports out of a Bend source file.
//
// The loader reads the imports at the top of a file and stops at the
// first line that is not one, so this does the same: a file's
// dependencies are the pinned hub packages it names.

/** A pinned hub package: a name and a four-number version. */
export type Pinned = {
  name: string;
  version: string;
};

const IMPORT = /^import\s+(\S+)(?:\s+as\s+([A-Za-z_]\w*))?\s*(?:#.*)?$/;
const HUB = /^([a-z][a-z0-9-]{0,63})@((?:0|[1-9][0-9]*)(?:\.(?:0|[1-9][0-9]*)){3})\//;

/**
 * imports reads the pinned hub packages a source file depends on. It
 * returns them in the order they appear, without repeats.
 */
export function imports(text: string): Pinned[] {
  const out: Pinned[] = [];
  const seen = new Set<string>();
  for (const raw of text.split("\n")) {
    const line = raw.trim();
    if (line === "" || line.startsWith("#")) continue;
    if (!/^import(\s|$)/.test(line)) break;
    const m = IMPORT.exec(line);
    if (m === null) continue;
    const h = HUB.exec(m[1]!);
    if (h === null) continue;
    const key = h[1] + "@" + h[2];
    if (seen.has(key)) continue;
    seen.add(key);
    out.push({ name: h[1]!, version: h[2]! });
  }
  return out;
}
