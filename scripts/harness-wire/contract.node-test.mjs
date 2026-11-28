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
    fs.writeFileSync(path.join(out, ".corpus.lock"), "");
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
    assert.match(run.stderr, /another capture or --resanitize is running/);
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
    assert.equal(fs.existsSync(path.join(out, ".corpus.lock")), false);
  } finally {
    fs.rmSync(out, { recursive: true, force: true });
  }
});

test("every value in the committed corpus is one a person reviewed", async () => {
  const { unreviewedCorpus } = await import("./allowed.mjs");
  // Every folder of the corpus, a new harness's included.
  assert.deepEqual(
    unreviewedCorpus(),
    [],
    "run allowed.mjs, review, then --write",
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
    write({
      type: "ok",
      account: { type: "chatgpt" },
      n: 1,
      models: [
        { value: "model-1", isDefault: true },
        { value: "model-2", isDefault: false },
      ],
      processId: "process-1",
    });
    writeReviewed("x", corpus, root);
    assert.deepEqual(unreviewed("x", corpus, root), []);
    write({
      type: "someone@private",
      account: { type: "chatgpt", plan: "pro" },
      n: 2,
      usage: { output_tokens: 7 },
      models: [
        { value: "model-1", isDefault: false },
        { value: "model-2", isDefault: true },
      ],
      processId: "28220",
    });
    const problems = unreviewed("x", corpus, root).join("\n");
    assert.match(
      problems,
      /t\.jsonl:1\.frame\.type: string "someone@private" is not reviewed/,
    );
    assert.match(
      problems,
      /t\.jsonl:1\.frame\.account\.plan: account path "account:plan" is not reviewed/,
    );
    assert.match(
      problems,
      /t\.jsonl:1\.frame\.n: number under n 2 is not reviewed/,
    );
    assert.match(
      problems,
      /output_tokens: 7 is neither 0 nor output_tokens's placeholder 50/,
    );
    // The default moved to another model: same values by key, new flags.
    assert.match(
      problems,
      /flag "t\.jsonl:1\.frame\.models\[0\]\.isDefault=false" is not reviewed/,
    );
    assert.match(problems, /"28220" is not a synthetic process handle/);
  } finally {
    fs.rmSync(base, { recursive: true, force: true });
  }
});

test("moving which Codex model is the default fails the reviewed-value check", async () => {
  const { unreviewed, writeReviewed } = await import("./allowed.mjs");
  const base = fs.mkdtempSync(path.join(os.tmpdir(), "sage-allowed-codex-"));
  try {
    const corpus = path.join(base, "corpus");
    const root = path.join(base, "contracts");
    fs.mkdirSync(path.join(corpus, "x"), { recursive: true });
    fs.mkdirSync(path.join(root, "x"), { recursive: true });
    const write = (first, second) =>
      fs.writeFileSync(
        path.join(corpus, "x", "t.jsonl"),
        `${JSON.stringify({
          direction: "in",
          frame: {
            id: 3,
            result: {
              data: [
                {
                  id: "model-1",
                  model: "model-1",
                  displayName: "Model 1",
                  isDefault: first,
                },
                {
                  id: "model-2",
                  model: "model-2",
                  displayName: "Model 2",
                  isDefault: second,
                },
              ],
            },
          },
        })}\n`,
      );
    write(true, false);
    writeReviewed("x", corpus, root);
    write(false, true);
    assert.match(
      unreviewed("x", corpus, root).join("\n"),
      /flag "t\.jsonl:1\.frame\.result\.data\[0\]\.isDefault=false" is not reviewed/,
    );
  } finally {
    fs.rmSync(base, { recursive: true, force: true });
  }
});

test("pointing a model alias at another reviewed model fails the reviewed-value check", async () => {
  const { unreviewed, writeReviewed } = await import("./allowed.mjs");
  const base = fs.mkdtempSync(path.join(os.tmpdir(), "sage-allowed-alias-"));
  try {
    const corpus = path.join(base, "corpus");
    const root = path.join(base, "contracts");
    fs.mkdirSync(path.join(corpus, "x"), { recursive: true });
    fs.mkdirSync(path.join(root, "x"), { recursive: true });
    const write = (resolved) =>
      fs.writeFileSync(
        path.join(corpus, "x", "t.jsonl"),
        `${JSON.stringify({
          direction: "in",
          frame: {
            models: [
              { value: "default", resolvedModel: resolved },
              { value: "model-1", resolvedModel: "model-1" },
              { value: "model-3", resolvedModel: "model-3" },
            ],
          },
        })}\n`,
      );
    write("model-1");
    writeReviewed("x", corpus, root);
    write("model-3");
    assert.match(
      unreviewed("x", corpus, root).join("\n"),
      /placeholder "t\.jsonl:1\.frame\.models\[0\]\.resolvedModel=model-3" is not reviewed/,
    );
  } finally {
    fs.rmSync(base, { recursive: true, force: true });
  }
});

test("a model added to a usage map fails the reviewed-value check", async () => {
  const { unreviewed, writeReviewed } = await import("./allowed.mjs");
  const base = fs.mkdtempSync(path.join(os.tmpdir(), "sage-allowed-usage-"));
  try {
    const corpus = path.join(base, "corpus");
    const root = path.join(base, "contracts");
    fs.mkdirSync(path.join(corpus, "x"), { recursive: true });
    fs.mkdirSync(path.join(root, "x"), { recursive: true });
    const write = (usage) =>
      fs.writeFileSync(
        path.join(corpus, "x", "t.jsonl"),
        `${JSON.stringify({ direction: "in", frame: { modelUsage: usage } })}\n`,
      );
    write({ "model-5": { contextWindow: 1 }, "model-4": { contextWindow: 1 } });
    writeReviewed("x", corpus, root);
    write({ "model-5": { contextWindow: 1 } });
    writeReviewed("x", corpus, root);
    write({ "model-5": { contextWindow: 1 }, "model-4": { contextWindow: 1 } });
    assert.match(
      unreviewed("x", corpus, root).join("\n"),
      /placeholder "t\.jsonl:1\.frame\.modelUsage\{model-4\}" is not reviewed/,
    );
  } finally {
    fs.rmSync(base, { recursive: true, force: true });
  }
});

test("a value swapped between positions, or an unexpected file, fails the reviewed-value check", async () => {
  const { unreviewed, writeReviewed } = await import("./allowed.mjs");
  const base = fs.mkdtempSync(path.join(os.tmpdir(), "sage-allowed-swap-"));
  try {
    const corpus = path.join(base, "corpus");
    const root = path.join(base, "contracts");
    fs.mkdirSync(path.join(corpus, "x"), { recursive: true });
    fs.mkdirSync(path.join(root, "x"), { recursive: true });
    const write = (primary, secondary) =>
      fs.writeFileSync(
        path.join(corpus, "x", "t.jsonl"),
        `${JSON.stringify({
          direction: "in",
          frame: {
            rateLimits: {
              primary: { windowDurationMins: primary },
              secondary: { windowDurationMins: secondary },
            },
          },
        })}\n`,
      );
    write(300, 10080);
    writeReviewed("x", corpus, root);
    assert.deepEqual(unreviewed("x", corpus, root), []);
    write(10080, 300);
    assert.deepEqual(unreviewed("x", corpus, root), [
      "x/t.jsonl: changed since it was reviewed",
    ]);
    write(300, 10080);
    fs.writeFileSync(path.join(corpus, "x", "notes.txt"), "someone@private\n");
    assert.deepEqual(unreviewed("x", corpus, root), [
      "x/notes.txt: is not a reviewed file",
    ]);
  } finally {
    fs.rmSync(base, { recursive: true, force: true });
  }
});

test("a nested folder, a stray root file or a new harness folder fails the reviewed-value check", async () => {
  const { unreviewedCorpus, writeReviewed } = await import("./allowed.mjs");
  const base = fs.mkdtempSync(path.join(os.tmpdir(), "sage-allowed-tree-"));
  try {
    const corpus = path.join(base, "corpus");
    const root = path.join(base, "contracts");
    fs.mkdirSync(path.join(corpus, "x"), { recursive: true });
    fs.mkdirSync(path.join(root, "x"), { recursive: true });
    fs.writeFileSync(
      path.join(corpus, "x", "t.jsonl"),
      `${JSON.stringify({ direction: "in", frame: { type: "ok" } })}\n`,
    );
    writeReviewed("x", corpus, root);
    assert.deepEqual(unreviewedCorpus(corpus, root), []);
    fs.mkdirSync(path.join(corpus, "x", "nested"));
    fs.writeFileSync(path.join(corpus, "x", "nested", "a.json"), "{}");
    assert.throws(
      () => writeReviewed("x", corpus, root),
      /nested: not regular files, so no value was read/,
    );
    fs.writeFileSync(path.join(corpus, "notes.txt"), "someone@private\n");
    fs.mkdirSync(path.join(corpus, "y"));
    assert.deepEqual(unreviewedCorpus(corpus, root), [
      "notes.txt: is not a harness folder; the corpus root holds those only",
      "x/nested: not regular files, so no value was read",
      "x/nested: is not a file; a corpus folder holds files only",
      "x/nested: is not a reviewed file",
      "y: no corpus-values.json",
    ]);
  } finally {
    fs.rmSync(base, { recursive: true, force: true });
  }
});

test("a reference may only name commands.jsonl by a 16-hex digest", async () => {
  const { readTranscript } = await import("./shape.mjs");
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "sage-ref-"));
  try {
    fs.writeFileSync(
      path.join(dir, "commands.jsonl"),
      `${JSON.stringify({ frame: { a: 1 } })}\n`,
    );
    for (const ref of [
      { ref: "../../outside.jsonl", sha256: "0123456789abcdef" },
      { ref: "commands.jsonl", sha256: "" },
      { ref: "commands.jsonl", sha256: "0123" },
    ]) {
      fs.writeFileSync(
        path.join(dir, "t.jsonl"),
        `${JSON.stringify({ direction: "in", ...ref })}\n`,
      );
      assert.throws(
        () => readTranscript(path.join(dir, "t.jsonl")),
        /must be commands\.jsonl by a 16-hex digest/,
      );
    }
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("an interrupted --resanitize that had replaced every file says so, not to restore", () => {
  const out = fs.mkdtempSync(path.join(os.tmpdir(), "sage-resanitize-done-"));
  try {
    fs.mkdirSync(path.join(out, "pi"));
    fs.writeFileSync(path.join(out, "pi", "edit.jsonl.123-0a1b2c3d.bak"), "");
    fs.writeFileSync(path.join(out, ".resanitize-123-0a1b2c3d.committed"), "");
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
    assert.match(run.stderr, /had replaced every file/);
    assert.doesNotMatch(run.stderr, /put each \.bak back/);
  } finally {
    fs.rmSync(out, { recursive: true, force: true });
  }
});
