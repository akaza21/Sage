#!/usr/bin/env node
// The corpus privacy property: every value in a committed harness-wire
// corpus is one a person reviewed: whole records (envelope and frame,
// references resolved) and each meta.json. Each harness's reviewed set is
// harness-contracts/<harness>/corpus-values.json:
//   strings       every string value (scripted prompts and replies, enums,
//                 kinds, placeholders, fixture paths)
//   keys          every object key
//   numbers       every number, by the key it sits under
//   flags         every boolean, where it is (file:line and indexed path)
//   placeholders  every pseudonym (model-N, command-N, ...), where it is
//   contextPaths  every key path under an account, plan, usage, model,
//                 catalog or org object, as `<root>:<path>`
// A number under a scrubbed key (tokens, cost, usage, pid) must also equal
// its placeholder. Run with --write only to record a reviewed change.
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { CONTRACTS, CORPUS } from "./contract.mjs";
import {
  ACCOUNT_RULES,
  CATALOGS,
  COST_KEYS,
  MODEL_KEYED,
  MODEL_LISTS,
  PLACEHOLDER,
  PROCESS_HANDLE,
  PROCESS_KEYS,
  TOKENS,
  tokenCount,
} from "./sanitize.mjs";
import { readTranscript } from "./shape.mjs";

// Objects whose keys say something about the account.
const CONTEXTS = new Set(
  [
    ...ACCOUNT_RULES.keys(),
    ...CATALOGS.keys(),
    ...MODEL_LISTS,
    ...MODEL_KEYED,
  ].filter((root) => !root.startsWith("@")),
);
// Scrubbed numbers and the one value each may hold.
const FIXED = [
  [(key) => TOKENS.test(key), (key) => tokenCount(key)],
  [(key) => COST_KEYS.has(key), () => 0.01],
  [(key) => key === "pid", () => 0],
  [(key) => key === "usedPercent", () => 10],
  [(key) => key === "utilization", () => 0.1],
];
const VALUES_FILE = "corpus-values.json";

// Every value of one harness corpus, each with the first path it was seen at.
export function corpusValues(dir) {
  const found = {
    strings: new Map(),
    keys: new Map(),
    numbers: new Map(),
    flags: new Map(),
    placeholders: new Map(),
    contextPaths: new Map(),
    fixed: [],
  };
  const note = (map, value, at) => {
    if (!map.has(value)) map.set(value, at);
  };
  let detecting = true;
  const byKey = (map, key, value, at) => {
    if (!map.has(key)) map.set(key, new Map());
    note(map.get(key), value, at);
  };
  const walk = (value, at, key, context) => {
    if (
      PROCESS_KEYS.has(key) &&
      value !== null &&
      !PROCESS_HANDLE.test(String(value))
    )
      found.fixed.push(
        `${at}: ${JSON.stringify(value)} is not a synthetic process handle`,
      );
    if (typeof value === "string") {
      note(found.strings, value, at);
      // A pseudonym says which model, command or id a field names, so moving
      // one (pointing an alias at another reviewed model) fails too.
      if (PLACEHOLDER.test(value) && value !== "redacted")
        note(found.placeholders, `${at}=${value}`, at);
    }
    // Which model is the default, or which entry is hidden or enabled, can
    // be the account's own wherever it is written: every boolean is
    // reviewed where it is (file:line and indexed path), so moving one fails.
    else if (typeof value === "boolean")
      note(found.flags, `${at}=${value}`, at);
    else if (typeof value === "number") {
      const rule = FIXED.find(([applies]) => applies(key));
      // 0 is the neutral value an account allow-list writes.
      if (rule && value !== 0 && value !== rule[1](key))
        found.fixed.push(
          `${at}: ${value} is neither 0 nor ${key}'s placeholder ${rule[1](key)}`,
        );
      byKey(found.numbers, key, value, at);
    } else if (Array.isArray(value))
      value.forEach((entry, index) =>
        walk(
          entry,
          `${at}[${index}]`,
          key,
          context && { ...context, path: `${context.path}[]` },
        ),
      );
    else if (value && typeof value === "object")
      for (const [name, entry] of Object.entries(value)) {
        note(found.keys, name, at);
        const shown = PLACEHOLDER.test(name) ? "*" : name;
        const inner = context
          ? {
              root: context.root,
              path: context.path ? `${context.path}.${shown}` : shown,
            }
          : detecting && CONTEXTS.has(name)
            ? { root: name, path: "" }
            : null;
        if (inner && inner.path)
          note(
            found.contextPaths,
            `${inner.root}:${inner.path}`,
            `${at}.${name}`,
          );
        walk(entry, `${at}.${name}`, name, inner);
      }
  };
  for (const name of fs
    .readdirSync(dir)
    .filter((entry) => entry.endsWith(".jsonl"))
    .sort())
    readTranscript(path.join(dir, name)).forEach((record, line) =>
      walk(record, `${name}:${line + 1}`, "", null),
    );
  const metaFile = path.join(dir, "meta.json");
  if (fs.existsSync(metaFile)) {
    // A capture's own notes name scenarios, not account objects.
    detecting = false;
    walk(JSON.parse(fs.readFileSync(metaFile, "utf8")), "meta.json", "", null);
  }
  return found;
}

const sorted = (values) =>
  [...values].sort((a, b) =>
    typeof a === "number" ? a - b : String(a).localeCompare(String(b)),
  );

function serialize(found) {
  return {
    strings: sorted(found.strings.keys()),
    keys: sorted(found.keys.keys()),
    numbers: Object.fromEntries(
      sorted(found.numbers.keys()).map((key) => [
        key,
        sorted(found.numbers.get(key).keys()),
      ]),
    ),
    flags: sorted(found.flags.keys()),
    placeholders: sorted(found.placeholders.keys()),
    contextPaths: sorted(found.contextPaths.keys()),
  };
}

// Every value of the corpus outside its reviewed set, with where it is.
export function unreviewed(harness, corpus = CORPUS, root = CONTRACTS) {
  const found = corpusValues(path.join(corpus, harness));
  const file = path.join(root, harness, VALUES_FILE);
  if (!fs.existsSync(file)) return [`${harness}: no ${VALUES_FILE}`];
  const allowed = JSON.parse(fs.readFileSync(file, "utf8"));
  const out = found.fixed.map((problem) => `${harness}/${problem}`);
  const missing = (label, map, set) => {
    for (const [value, at] of map)
      if (!set.has(value))
        out.push(
          `${harness}/${at}: ${label} ${JSON.stringify(value).slice(0, 120)} is not reviewed`,
        );
  };
  missing("string", found.strings, new Set(allowed.strings));
  missing("key", found.keys, new Set(allowed.keys));
  missing("account path", found.contextPaths, new Set(allowed.contextPaths));
  missing("flag", found.flags, new Set(allowed.flags ?? []));
  missing(
    "placeholder",
    found.placeholders,
    new Set(allowed.placeholders ?? []),
  );
  for (const [key, values] of found.numbers)
    missing(`number under ${key}`, values, new Set(allowed.numbers[key] ?? []));
  return out;
}

export function writeReviewed(harness, corpus = CORPUS, root = CONTRACTS) {
  const found = corpusValues(path.join(corpus, harness));
  if (found.fixed.length) throw new Error(found.fixed.join("\n"));
  fs.writeFileSync(
    path.join(root, harness, VALUES_FILE),
    `${JSON.stringify(serialize(found), null, 2)}\n`,
  );
}

function main() {
  const harnesses = fs
    .readdirSync(CORPUS)
    .filter((name) => fs.statSync(path.join(CORPUS, name)).isDirectory())
    .sort();
  if (process.argv.includes("--write")) {
    harnesses.forEach((harness) => writeReviewed(harness));
    return;
  }
  const problems = harnesses.flatMap((harness) => unreviewed(harness));
  for (const problem of problems.slice(0, 50)) console.error(problem);
  if (problems.length) {
    console.error(`${problems.length} unreviewed value(s)`);
    process.exit(1);
  }
  console.log("every corpus value is reviewed");
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) main();
