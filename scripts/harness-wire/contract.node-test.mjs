import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  CONTRACTS,
  checkCorpus,
  contracts,
  diffContracts,
} from "./contract.mjs";
import { neutralizeUndeclared } from "./neutral.mjs";

test("every recorded frame matches its harness version's contract and reviewed list", () => {
  const checked = fs
    .readdirSync(CONTRACTS)
    .filter((harness) => contracts(harness).length);
  assert.ok(checked.length >= 2, `contracts found for ${checked}`);
  assert.deepEqual(checkCorpus(), []);
});

test("an upgrade reports added and removed kinds and fields", () => {
  const contract = (version, init, types) => ({
    harness: "claude",
    version,
    in: { "system/init": ["Init"], ...init },
    out: {},
    types: { Init: types, Plugin: { name: [] } },
  });
  const before = contract(
    "2.1.330",
    { keep_alive: ["Alive"] },
    { model: [], plugins: ["Plugin"] },
  );
  before.types.Alive = { type: [] };
  const after = contract(
    "2.1.332",
    {},
    { model: [], is_backgrounded: [], plugins: ["*"] },
  );
  assert.deepEqual(diffContracts(before, after), [
    "protocol changes in claude 2.1.330 -> 2.1.332",
    "in removed keep_alive",
    "in system/init: - plugins.name",
    "in system/init: + is_backgrounded",
  ]);
});

test("a field a harness sends beyond its definitions keeps its key, not its value, unless Sage reads it", () => {
  const paths = new Map([
    [
      "in system/init",
      [["served_by_anthropic"], ["memory_paths"], ["plugins[]", "source"]],
    ],
  ]);
  const [record] = neutralizeUndeclared(
    [
      {
        direction: "in",
        frame: {
          type: "system",
          subtype: "init",
          served_by_anthropic: true,
          memory_paths: { auto: "C:/private" },
          plugins: [{ name: "p", source: "acme-marketplace" }],
          model: "m",
        },
      },
    ],
    paths,
  );
  assert.deepEqual(record.frame, {
    type: "system",
    subtype: "init",
    served_by_anthropic: false,
    memory_paths: { auto: "redacted" },
    plugins: [{ name: "p", source: "redacted" }],
    model: "m",
  });
});
test("an undeclared field Sage reads that stops arriving fails, even after a bless", () => {
  const base = fs.mkdtempSync(path.join(os.tmpdir(), "sage-read-by-sage-"));
  try {
    const corpus = path.join(base, "corpus");
    const root = path.join(base, "contracts");
    fs.mkdirSync(path.join(corpus, "x"), { recursive: true });
    fs.mkdirSync(path.join(root, "x"), { recursive: true });
    fs.writeFileSync(
      path.join(corpus, "x", "meta.json"),
      JSON.stringify({ version: "1.0.0" }),
    );
    fs.writeFileSync(
      path.join(corpus, "x", "commands.jsonl"),
      `${JSON.stringify({ direction: "in", frame: { type: "ping", extra: 1 } })}\n`,
    );
    fs.writeFileSync(
      path.join(root, "x", "1.0.0.json"),
      JSON.stringify({
        harness: "x",
        version: "1.0.0",
        in: { ping: ["P"] },
        out: {},
        types: { P: { type: [] } },
      }),
    );
    const gone = "in ping: + mode";
    fs.writeFileSync(
      path.join(root, "x", "undeclared.json"),
      JSON.stringify({
        version: "1.0.0",
        fields: ["in ping: + extra"],
        readBySage: [gone],
      }),
    );
    checkCorpus(corpus, { bless: true, root });
    const kept = JSON.parse(
      fs.readFileSync(path.join(root, "x", "undeclared.json"), "utf8"),
    );
    assert.deepEqual(kept.readBySage, [gone]);
    assert.deepEqual(checkCorpus(corpus, { root }), [
      `x 1.0.0 read by Sage, no longer sent: ${gone}`,
    ]);
  } finally {
    fs.rmSync(base, { recursive: true, force: true });
  }
});
