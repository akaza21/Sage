// Allow-list scrubbing for a committed corpus. A value Sage's replays do not
// need becomes a fixed placeholder valid for its type, so nothing the
// capturing account was sent survives by accident: the values of fields a
// harness sends beyond its own published definitions (its reviewed
// `undeclared.json`, minus `readBySage`), and every value inside an account,
// billing, rate-limit, plan, organization or model-catalog object except the
// keys a binding reads.
import fs from "node:fs";
import path from "node:path";

import { CONTRACTS, kinded } from "./contract.mjs";
// A fixed value of the same type: "redacted", 0, false, or the same
// structure under stable pseudonymous keys (`field-N`, one map per `keys`).
// Only what `synthetic` vouches for (a value the sanitizer wrote, so an id
// that links frames) keeps its form; with no voucher, nothing does.
export function neutral(
  value,
  keys = new Map(),
  synthetic = (candidate) => candidate === "redacted",
) {
  if (typeof value === "string") return synthetic(value) ? value : "redacted";
  if (typeof value === "number") return 0;
  if (typeof value === "boolean") return false;
  if (Array.isArray(value))
    return value.map((entry) => neutral(entry, keys, synthetic));
  if (value && typeof value === "object")
    return Object.fromEntries(
      Object.entries(value).map(([key, entry]) => {
        const own = [...keys.values()].includes(key);
        if (!synthetic(key) && !own && !keys.has(key))
          keys.set(key, `field-${keys.size + 1}`);
        return [
          synthetic(key) || own ? key : keys.get(key),
          neutral(entry, keys, synthetic),
        ];
      }),
    );
  return value;
}

// The reviewed undeclared field paths of `harness`, by `<direction> <kind>`.
export function undeclaredPaths(harness, root = CONTRACTS) {
  const file = path.join(root, harness, "undeclared.json");
  const paths = new Map();
  if (!fs.existsSync(file)) return paths;
  const { fields = [], readBySage = [] } = JSON.parse(
    fs.readFileSync(file, "utf8"),
  );
  const read = new Set(readBySage);
  for (const line of fields) {
    const match = /^(in|out) (.+): \+ (.+)$/.exec(line);
    if (!match || read.has(line)) continue;
    const key = `${match[1]} ${match[2]}`;
    if (!paths.has(key)) paths.set(key, []);
    paths.get(key).push(match[3].split("."));
  }
  return paths;
}

function neutralizeAt(value, segments, keys, synthetic) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return;
  const [segment, ...rest] = segments;
  const many = segment.endsWith("[]");
  const key = many ? segment.slice(0, -2) : segment;
  if (!(key in value)) return;
  if (!many) {
    if (rest.length) neutralizeAt(value[key], rest, keys, synthetic);
    else value[key] = neutral(value[key], keys, synthetic);
    return;
  }
  if (!Array.isArray(value[key])) return;
  if (rest.length)
    value[key].forEach((entry) => neutralizeAt(entry, rest, keys, synthetic));
  else value[key] = neutral(value[key], keys, synthetic);
}

// Neutralizes, in place, every undeclared field of each record's frame.
// A record that refers to a stored line (`{ ref, sha256 }`) takes part in
// pairing requests with replies through `resolve(record)`, its stored frame,
// so every inline reply gets its right kind; the stored line itself is
// neutralized where it is stored.
export function neutralizeUndeclared(
  records,
  paths,
  resolve = () => undefined,
  synthetic = (candidate) => candidate === "redacted",
) {
  if (!paths.size) return records;
  const framed = records
    .map((record) => ({
      record,
      direction: record.direction,
      frame: record.frame ?? (record.ref ? resolve(record) : undefined),
    }))
    .filter((entry) => entry.frame);
  const keys = new Map();
  [...kinded(framed)].forEach(({ direction, kind }, at) => {
    if (!framed[at].record.frame) return;
    for (const segments of paths.get(`${direction} ${kind}`) ?? [])
      neutralizeAt(framed[at].frame, segments, keys, synthetic);
  });
  return records;
}
