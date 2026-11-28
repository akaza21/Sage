#!/usr/bin/env node
// A harness's wire contract: every frame kind its official protocol
// definitions allow in each direction (named as `shape.mjs` names recorded
// frames), each pointing at the object types it may be, and those types as a
// graph `{ type id: { field: [child type ids] } }`. A field list holding `*`
// admits any value; a type with a `*` field admits any extra field. One file
// per checked harness version, generated from the harness's own published
// definitions by its provider (`<harness>.contract.mjs`), never edited.
//
//   node scripts/harness-wire/contract.mjs generate <harness> <version>
//   node scripts/harness-wire/contract.mjs check [corpus] [--bless]
//   node scripts/harness-wire/contract.mjs diff <before.json> <after.json>
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import * as prettier from "prettier";

import { OPAQUE, frameKind, readTranscript } from "./shape.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
export const CORPUS = path.resolve(
  HERE,
  "../../src-tauri/tests/fixtures/harness-wire",
);
export const CONTRACTS = path.resolve(
  HERE,
  "../../src-tauri/tests/fixtures/harness-contracts",
);
// Contracts kept per harness, and the largest one admitted.
export const KEPT = 3;
const MAX_BYTES = 512 * 1024;

export function release(text) {
  const match = /(\d+)\.(\d+)\.(\d+)/.exec(text ?? "");
  if (!match) throw new Error(`no x.y.z version in ${JSON.stringify(text)}`);
  return match.slice(1, 4).map(Number);
}

function compare(a, b) {
  return a[0] - b[0] || a[1] - b[1] || a[2] - b[2];
}

export function contracts(harness, root = CONTRACTS) {
  const dir = path.join(root, harness);
  if (!fs.existsSync(dir)) return [];
  return fs
    .readdirSync(dir)
    .filter((name) => /^\d+\.\d+\.\d+\.json$/.test(name))
    .map((name) => ({ version: release(name), file: path.join(dir, name) }))
    .sort((a, b) => compare(a.version, b.version));
}

// The newest contract at or below `version`.
export function contractFor(harness, version, root = CONTRACTS) {
  const wanted = release(version);
  const found = contracts(harness, root)
    .filter((entry) => compare(entry.version, wanted) <= 0)
    .at(-1);
  if (!found)
    throw new Error(
      `${harness} ${version} is older than every checked contract`,
    );
  return JSON.parse(fs.readFileSync(found.file, "utf8"));
}

// Each frame with its kind. A reply is named by the request it answers,
// which the other side sent.
export function* kinded(records) {
  const sent = { in: new Map(), out: new Map() };
  for (const { direction, frame } of records) {
    const mine = direction === "out" ? sent.out : sent.in;
    yield {
      direction,
      frame,
      kind: frameKind(frame, direction === "out" ? sent.in : sent.out),
    };
    const id = frame.request_id ?? frame.id;
    const name = frame.method ?? frame.request?.subtype ?? frame.type;
    if (id !== undefined && name) mine.set(String(id), name);
  }
}

// The fields of `value` the types `ids` do not define, as paths.
export function undefinedFields(types, ids, value, prefix = "", out = []) {
  if (ids.includes("*")) return out;
  if (Array.isArray(value)) {
    for (const entry of value)
      undefinedFields(types, ids, entry, `${prefix}[]`, out);
    return out;
  }
  if (!value || typeof value !== "object" || !ids.length) return out;
  const nodes = ids.map((id) => types[id]);
  for (const [key, entry] of Object.entries(value)) {
    const name = prefix ? `${prefix}.${key}` : key;
    const holders = nodes.filter((node) => key in node);
    if (!holders.length) {
      if (!nodes.some((node) => "*" in node)) out.push(name);
      continue;
    }
    if (OPAQUE.has(key)) continue;
    undefinedFields(
      types,
      [...new Set(holders.flatMap((node) => node[key]))],
      entry,
      name,
      out,
    );
  }
  return out;
}

// Frames the contract does not define, as sorted report lines.
export function violations(contract, records, label) {
  const lines = new Set();
  for (const { direction, frame, kind } of kinded(records)) {
    const ids = contract[direction][kind];
    if (!ids) {
      lines.add(
        `${label} ${direction} ${kind}: kind not in the protocol definitions`,
      );
      continue;
    }
    for (const field of undefinedFields(contract.types, ids, frame))
      lines.add(`${label} ${direction} ${kind}: + ${field}`);
  }
  return [...lines].sort();
}

// A kind's field paths, for reading a protocol change; recursion stops at a
// type already on the path.
function paths(types, ids, prefix = "", seen = new Set(), out = new Set()) {
  for (const id of ids) {
    if (id === "*" || seen.has(id)) continue;
    for (const [key, children] of Object.entries(types[id])) {
      const name = prefix ? `${prefix}.${key}` : key;
      out.add(name);
      paths(types, children, name, new Set(seen).add(id), out);
    }
  }
  return out;
}

// Every protocol change between two contracts, as report lines.
export function diffContracts(before, after) {
  const lines = [
    `protocol changes in ${after.harness} ${before.version} -> ${after.version}`,
  ];
  for (const direction of ["in", "out"]) {
    const was = before[direction];
    const is = after[direction];
    for (const kind of Object.keys(was))
      if (!(kind in is)) lines.push(`${direction} removed ${kind}`);
    for (const kind of Object.keys(is)) {
      if (!(kind in was)) {
        lines.push(`${direction} added ${kind}`);
        continue;
      }
      const old = paths(before.types, was[kind]);
      const now = paths(after.types, is[kind]);
      for (const field of old)
        if (!now.has(field)) lines.push(`${direction} ${kind}: - ${field}`);
      for (const field of now)
        if (!old.has(field)) lines.push(`${direction} ${kind}: + ${field}`);
    }
  }
  return lines;
}

function sorted(object) {
  return Object.fromEntries(
    Object.keys(object)
      .sort()
      .map((key) => [key, object[key]]),
  );
}

export async function generate(harness, version, root = CONTRACTS) {
  const provider = await import(
    pathToFileURL(path.join(HERE, `${harness}.contract.mjs`)).href
  );
  const work = fs.mkdtempSync(
    path.join(os.tmpdir(), `sage-contract-${harness}-`),
  );
  try {
    const found = await provider.contract(version, work);
    const contract = {
      harness,
      version: release(found.version).join("."),
      source: found.source,
      in: sorted(found.in),
      out: sorted(found.out),
      types: Object.fromEntries(
        Object.keys(found.types)
          .sort()
          .map((id) => [id, sorted(found.types[id])]),
      ),
    };
    const dir = path.join(root, harness);
    fs.mkdirSync(dir, { recursive: true });
    const file = path.join(dir, `${contract.version}.json`);
    const text = await prettier.format(JSON.stringify(contract), {
      ...(await prettier.resolveConfig(file)),
      filepath: file,
    });
    if (text.length > MAX_BYTES)
      throw new Error(
        `${harness} contract is ${text.length} bytes, over ${MAX_BYTES}`,
      );
    fs.writeFileSync(file, text);
    for (const old of contracts(harness, root).slice(0, -KEPT))
      fs.rmSync(old.file);
    return { contract, file };
  } finally {
    fs.rmSync(work, { recursive: true, force: true });
  }
}

// What each harness with contracts sent beyond its recorded version's
// contract, against the reviewed list in `harness-contracts/<harness>/undeclared.json`: a
// field the harness sends that its own definitions do not declare. Returns
// report lines for every difference; `bless` rewrites the lists instead.
export function checkCorpus(
  corpus = CORPUS,
  { bless = false, root = CONTRACTS } = {},
) {
  const lines = [];
  for (const harness of fs.readdirSync(corpus).sort()) {
    if (!contracts(harness, root).length) continue;
    const dir = path.join(corpus, harness);
    const meta = JSON.parse(
      fs.readFileSync(path.join(dir, "meta.json"), "utf8"),
    );
    const contract = contractFor(harness, meta.version, root);
    const found = new Set();
    for (const name of fs
      .readdirSync(dir)
      .filter((entry) => entry.endsWith(".jsonl"))
      .sort())
      for (const line of violations(
        contract,
        readTranscript(path.join(dir, name)),
        "",
      ))
        found.add(line.trim());
    const file = path.join(root, harness, "undeclared.json");
    const reviewed = fs.existsSync(file)
      ? JSON.parse(fs.readFileSync(file, "utf8"))
      : { fields: [] };
    // `readBySage`: undeclared fields a binding reads, so a harness that
    // drops one breaks Sage without its definitions saying so. Kept by hand
    // across blesses; each must still be observed.
    const read = reviewed.readBySage ?? [];
    if (bless) {
      const listed = { version: contract.version, fields: [...found].sort() };
      if (read.length)
        listed.readBySage = read.filter((line) => found.has(line));
      fs.writeFileSync(file, `${JSON.stringify(listed, null, 2)}\n`);
      continue;
    }
    for (const line of read)
      if (!found.has(line))
        lines.push(
          `${harness} ${contract.version} read by Sage, no longer sent: ${line}`,
        );
    const known = new Set(reviewed.fields);
    if (reviewed.version !== contract.version)
      lines.push(
        `${harness}: undeclared.json is for ${reviewed.version}, the corpus for ${contract.version}`,
      );
    for (const line of found)
      if (!known.has(line))
        lines.push(`${harness} ${contract.version} new: ${line}`);
    for (const line of known)
      if (!found.has(line))
        lines.push(`${harness} ${contract.version} no longer seen: ${line}`);
  }
  return lines;
}

async function main() {
  const [mode, first, second] = process.argv.slice(2);
  if (mode === "generate" && first && second) {
    const { file } = await generate(first, second);
    console.log(`wrote ${path.relative(process.cwd(), file)}`);
  } else if (mode === "check") {
    const bless = process.argv.includes("--bless");
    const lines = checkCorpus(first && first !== "--bless" ? first : CORPUS, {
      bless,
    });
    for (const line of lines) console.log(line);
    if (lines.length) {
      console.log("triage each line, then rerun with --bless to admit it");
      process.exit(1);
    }
    console.log(
      bless
        ? "wrote undeclared.json"
        : "every recorded frame matches its contract",
    );
  } else if (mode === "diff" && first && second) {
    const read = (file) => JSON.parse(fs.readFileSync(file, "utf8"));
    for (const line of diffContracts(read(first), read(second)))
      console.log(line);
  } else {
    console.error(
      "usage: contract.mjs generate <harness> <version> | check [corpus] | diff <a> <b>",
    );
    process.exit(2);
  }
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) await main();
