#!/usr/bin/env node
// What changed in each harness's protocol since its newest checked contract.
// Generates the contract of the harness's newest published release (or the
// version given) from its official definitions and prints every added or
// removed kind and field. Exits 1 when a harness changed, so a scheduled run
// flags it. `--write` admits the new contract (the oldest beyond the kept
// three is dropped); then recapture the corpus at that version
// (`capture.mjs`), triage `shape.mjs --diff`, rerun `contract.mjs check` and
// the replay tests, and review both diffs in one PR.
//
//   node scripts/harness-wire/upgrade.mjs [harness[@version] ...] [--write]
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { CONTRACTS, contracts, diffContracts, generate } from "./contract.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));

// What a regenerated contract means against the checked one: its report
// lines, and whether the protocol changed. A newer release that changed
// nothing is not a change; the same release regenerating differently (a
// corrected provider, a mis-generated contract) is one.
export function report(before, after) {
  const [header, ...changes] = diffContracts(before, after);
  const same = before.version === after.version;
  if (!changes.length)
    return {
      changed: false,
      lines: [
        same
          ? `${after.harness}: no newer release`
          : `${header}: no protocol changes`,
      ],
    };
  return {
    changed: true,
    lines: [
      same
        ? `${after.harness} ${after.version}: the regenerated contract differs from the checked one`
        : header,
      ...changes,
    ],
  };
}

async function upgrade(target, write) {
  const [harness, asked] = target.split("@");
  const provider = await import(
    pathToFileURL(path.join(HERE, `${harness}.contract.mjs`)).href
  );
  const version = asked ?? provider.latest();
  const kept = contracts(harness).at(-1);
  if (!kept) throw new Error(`${harness}: no checked contract to compare`);
  const before = JSON.parse(fs.readFileSync(kept.file, "utf8"));
  const root = write
    ? CONTRACTS
    : fs.mkdtempSync(path.join(os.tmpdir(), "sage-upgrade-"));
  try {
    const { contract } = await generate(harness, version, root);
    return report(before, contract);
  } finally {
    if (!write) fs.rmSync(root, { recursive: true, force: true });
  }
}

async function main() {
  const args = process.argv.slice(2);
  const write = args.includes("--write");
  const named = args.filter((arg) => !arg.startsWith("--"));
  const targets = named.length
    ? named
    : fs
        .readdirSync(CONTRACTS)
        .filter((harness) => contracts(harness).length)
        .sort();
  let changed = false;
  for (const target of targets) {
    const result = await upgrade(target, write);
    for (const line of result.lines) console.log(line);
    changed ||= result.changed;
  }
  if (changed && !write) process.exit(1);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) await main();
