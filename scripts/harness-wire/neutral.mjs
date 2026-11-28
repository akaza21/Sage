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

const PLACEHOLDER = /^(?:redacted|model-\d+|Model \d+)$/;

// A fixed value of the same type: "redacted", 0, false, or the same structure.
export function neutral(value) {
  if (typeof value === "string")
    return PLACEHOLDER.test(value) ? value : "redacted";
  if (typeof value === "number") return 0;
  if (typeof value === "boolean") return false;
  if (Array.isArray(value)) return value.map(neutral);
  if (value && typeof value === "object")
    return Object.fromEntries(
      Object.entries(value).map(([key, entry]) => [key, neutral(entry)]),
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

function neutralizeAt(value, segments) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return;
  const [segment, ...rest] = segments;
  const many = segment.endsWith("[]");
  const key = many ? segment.slice(0, -2) : segment;
  if (!(key in value)) return;
  if (!many) {
    if (rest.length) neutralizeAt(value[key], rest);
    else value[key] = neutral(value[key]);
    return;
  }
  if (!Array.isArray(value[key])) return;
  if (rest.length) value[key].forEach((entry) => neutralizeAt(entry, rest));
  else value[key] = neutral(value[key]);
}

// Neutralizes, in place, every undeclared field of each record's frame.
export function neutralizeUndeclared(records, paths) {
  if (!paths.size) return records;
  const framed = records.filter((record) => record.frame);
  [...kinded(framed)].forEach(({ direction, kind }, at) => {
    for (const segments of paths.get(`${direction} ${kind}`) ?? [])
      neutralizeAt(framed[at].frame, segments);
  });
  return records;
}
