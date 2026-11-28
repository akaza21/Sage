import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
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
    "in system/init: + plugins.*",
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
    memory_paths: { "field-1": "redacted" },
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

test("a comparison past a hard cap stops loudly instead of enumerating", () => {
  const contract = (version, kinds, types) => ({
    harness: "pi",
    version,
    in: kinds,
    out: {},
    types,
  });
  const compare = (kinds, types) =>
    diffContracts(
      contract("1.5.0", kinds, types),
      contract("1.7.0", kinds, types),
    );
  // 100 kinds sharing one 14-level graph with two fields per level.
  const fan = { T14: { leaf: [] } };
  for (let level = 0; level < 14; level += 1)
    fan[`T${level}`] = { a: [`T${level + 1}`], b: [`T${level + 1}`] };
  const kinds = Object.fromEntries(
    Array.from({ length: 100 }, (_, index) => [`kind${index}`, ["T0"]]),
  );
  assert.throws(() => compare(kinds, fan), /field paths walked exceed 2000000/);
  // A chain nested deeper than any harness nests its fields.
  const chain = { C20: { leaf: [] } };
  for (let level = 0; level < 20; level += 1)
    chain[`C${level}`] = { next: [`C${level + 1}`] };
  assert.throws(
    () => compare({ start: ["C0"] }, chain),
    /nested fields exceed 16/,
  );
  // More changed kinds than a report holds.
  const added = Object.fromEntries(
    Array.from({ length: 5_000 }, (_, index) => [`new${index}`, ["*"]]),
  );
  assert.throws(
    () =>
      diffContracts(contract("1.5.0", {}, {}), contract("1.7.0", added, {})),
    /report lines exceed 5000/,
  );
});

test("a neutralized object keeps no key of its own, only pseudonyms and scrubbed ids", async () => {
  const { neutral } = await import("./neutral.mjs");
  assert.deepEqual(
    neutral({
      toolu_000000000000000000000002: { AcmeProject: "secret", count: 3 },
      "acme-key": [true],
    }),
    {
      toolu_000000000000000000000002: { "field-1": "redacted", "field-2": 0 },
      "field-3": [false],
    },
  );
});

test("a second --resanitize refuses to run while a corpus is locked", () => {
  const out = fs.mkdtempSync(path.join(os.tmpdir(), "sage-resanitize-lock-"));
  try {
    fs.writeFileSync(path.join(out, ".resanitize.lock"), "");
    const run = spawnSync(
      process.execPath,
      [
        "scripts/harness-wire/capture.mjs",
        "--out",
        out,
        "--harness",
        "pi",
        "--resanitize",
        "--work",
        out,
      ],
      { encoding: "utf8" },
    );
    assert.notEqual(run.status, 0);
    assert.match(run.stderr, /another --resanitize is running/);
  } finally {
    fs.rmSync(out, { recursive: true, force: true });
  }
});

test("a reply to a request stored by reference still gets its kind, so its undeclared fields are neutralized", () => {
  const request = {
    type: "control_request",
    request_id: "1",
    request: { subtype: "initialize" },
  };
  const paths = new Map([
    [
      "in control_response/success/initialize",
      [["response", "response", "analytics_disabled"]],
    ],
  ]);
  const records = neutralizeUndeclared(
    [
      { direction: "out", ref: "commands.jsonl", sha256: "abc" },
      {
        direction: "in",
        frame: {
          type: "control_response",
          response: {
            subtype: "success",
            request_id: "1",
            response: { analytics_disabled: true },
          },
        },
      },
    ],
    paths,
    (record) => (record.sha256 === "abc" ? request : undefined),
  );
  assert.equal(records[1].frame.response.response.analytics_disabled, false);
  assert.deepEqual(records[0], {
    direction: "out",
    ref: "commands.jsonl",
    sha256: "abc",
  });
});

test("--resanitize refuses to run over an interrupted run's backups", () => {
  const out = fs.mkdtempSync(path.join(os.tmpdir(), "sage-resanitize-left-"));
  try {
    fs.mkdirSync(path.join(out, "pi"));
    fs.writeFileSync(path.join(out, "pi", "edit.jsonl.123-0a1b2c3d.bak"), "");
    const run = spawnSync(
      process.execPath,
      [
        "scripts/harness-wire/capture.mjs",
        "--out",
        out,
        "--harness",
        "pi",
        "--resanitize",
        "--work",
        out,
      ],
      { encoding: "utf8" },
    );
    assert.notEqual(run.status, 0);
    assert.match(run.stderr, /interrupted --resanitize left/);
    assert.equal(fs.existsSync(path.join(out, ".resanitize.lock")), false);
  } finally {
    fs.rmSync(out, { recursive: true, force: true });
  }
});

test("every value in the committed corpus is one a person reviewed", async () => {
  const { unreviewed } = await import("./allowed.mjs");
  for (const harness of ["claude", "codex", "pi"])
    assert.deepEqual(
      unreviewed(harness),
      [],
      `${harness}: run allowed.mjs, review, then --write`,
    );
});

test("an unreviewed string, key, number or account path fails with its path", async () => {
  const { unreviewed, writeReviewed } = await import("./allowed.mjs");
  const base = fs.mkdtempSync(path.join(os.tmpdir(), "sage-allowed-"));
  try {
    const corpus = path.join(base, "corpus");
    const root = path.join(base, "contracts");
    fs.mkdirSync(path.join(corpus, "x"), { recursive: true });
    fs.mkdirSync(path.join(root, "x"), { recursive: true });
    const write = (frame) =>
      fs.writeFileSync(
        path.join(corpus, "x", "t.jsonl"),
        `${JSON.stringify({ direction: "in", frame })}\n`,
      );
    write({ type: "ok", account: { type: "chatgpt" }, n: 1 });
    writeReviewed("x", corpus, root);
    assert.deepEqual(unreviewed("x", corpus, root), []);
    write({
      type: "someone@private",
      account: { type: "chatgpt", plan: "pro" },
      n: 2,
      usage: { output_tokens: 7 },
    });
    const problems = unreviewed("x", corpus, root).join("\n");
    assert.match(
      problems,
      /t\.jsonl:1\.type: string "someone@private" is not reviewed/,
    );
    assert.match(
      problems,
      /t\.jsonl:1\.account\.plan: account path "account:plan" is not reviewed/,
    );
    assert.match(problems, /t\.jsonl:1\.n: number under n 2 is not reviewed/);
    assert.match(
      problems,
      /output_tokens: 7 is neither 0 nor output_tokens's placeholder 50/,
    );
  } finally {
    fs.rmSync(base, { recursive: true, force: true });
  }
});
