import assert from "node:assert/strict";
import test from "node:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import {
  MAX_BATCH,
  ledgerRecords,
  liveFrames,
  nextBatch,
  nextLedgerPath,
  pairRuns,
  parseRun,
  planRuns,
  readLedger,
  runBatch,
} from "./capture-native-chat-batch.mjs";
import { PARITY_STATE_NAMES } from "./capture-native-chat-parity.mjs";

const BUILD = "a".repeat(64);
const RUN_ID = "0c801b46-ed18-48d3-8da8-0204a2d2d0e4";

test("plans only CLI-admitted cells in catalogue order", () => {
  const parity = planRuns({ states: PARITY_STATE_NAMES });
  assert.equal(parity.length, 150);
  assert.equal(new Set(parity).size, parity.length);
  for (const line of parity) parseRun(line);
  assert.ok(parity.includes("codex:turn-navigator-hover:1888"));
  assert.ok(!parity.includes("codex:turn-navigator-hover:720"));
  assert.ok(!parity.some((line) => line.includes(":side-chat-running:")));
  assert.ok(!parity.some((line) => /^pi:fork-/u.test(line)));
  assert.ok(!parity.some((line) => /^claude:tool-mcp-/u.test(line)));
  assert.deepEqual(planRuns({ states: ["responsive-720"] }), [
    "pi:responsive-720:720",
    "codex:responsive-720:720",
    "claude:responsive-720:720",
  ]);
  assert.throws(() => parseRun("codex:turn-completed:1431"), /batch-run/u);
});

test("next resumes after captured cells of the same build and bounds attempts", () => {
  const order = ["pi:new-chat:1888", "pi:new-chat:720", "pi:new-chat:480"];
  const captured = (width, build = BUILD) => ({
    harness: "pi",
    state: "new-chat",
    width,
    result: {
      verdict: "captured",
      directory: "work/native-captures/" + RUN_ID,
    },
    build,
  });
  const probeFor = (record) => ({
    verdict: "captured",
    hashes: { build: record.build },
  });
  assert.deepEqual(
    nextBatch({ order, records: [captured(1888)], build: BUILD, probeFor }),
    ["pi:new-chat:720", "pi:new-chat:480"],
  );
  // A capture by another build is redone; a blocked attempt counts toward the cap.
  const blocked = {
    harness: "pi",
    state: "new-chat",
    width: 720,
    result: { verdict: "blocked" },
  };
  assert.deepEqual(
    nextBatch({
      order,
      records: [captured(1888, "b".repeat(64)), blocked],
      build: BUILD,
      probeFor,
      maxAttempts: 1,
    }),
    ["pi:new-chat:480"],
  );
  assert.deepEqual(
    nextBatch({
      order,
      records: [blocked],
      build: BUILD,
      probeFor,
      maxAttempts: 2,
      size: 1,
    }),
    ["pi:new-chat:1888"],
  );
  for (const size of [0, MAX_BATCH + 1])
    assert.throws(
      () => nextBatch({ order, records: [], build: BUILD, probeFor, size }),
      /batch-size/u,
    );
  assert.throws(
    () => nextBatch({ order, records: [], build: "x", probeFor }),
    /batch-build/u,
  );
});

test("a batch runs at most 30 cells, records each and stops on a failed preflight", async () => {
  const runs = ["pi:new-chat:1888", "pi:new-chat:720", "pi:new-chat:480"];
  const appended = [];
  let checks = 0;
  const result = await runBatch({
    runs,
    ledger: "unused",
    capture: async (run) => ({
      exit: 0,
      result: { verdict: "captured", run: RUN_ID, state: run.state },
    }),
    check: async () => (++checks === 3 ? "port 1420 busy" : null),
    append: (record) => appended.push(record),
    now: () => 1000,
  });
  assert.equal(result.aborted, "port 1420 busy before pi:new-chat:480");
  assert.deepEqual(
    appended.map(({ width }) => width),
    [1888, 720],
  );
  assert.deepEqual(Object.keys(appended[0]).sort(), [
    "exit",
    "harness",
    "result",
    "secs",
    "state",
    "width",
  ]);
  await assert.rejects(
    runBatch({
      runs: Array(MAX_BATCH + 1).fill(runs[0]),
      ledger: "unused",
      capture: async () => ({}),
      check: async () => null,
      append: () => {},
    }),
    /batch-size/u,
  );
});

test("ledgers resume across PowerShell BOMs and numbered files; pairs name live frames", () => {
  const root = mkdtempSync(path.join(process.cwd(), ".nc4-batch-test-"));
  try {
    mkdirSync(path.join(root, "work", "native-captures", RUN_ID), {
      recursive: true,
    });
    const record = {
      harness: "codex",
      state: "fork-dialog",
      width: 1888,
      exit: 0,
      result: {
        verdict: "captured",
        run: RUN_ID,
        directory: "work\\native-captures\\" + RUN_ID,
      },
    };
    writeFileSync(
      path.join(root, "work", "nc4-1.jsonl"),
      "\uFEFF" + JSON.stringify(record) + "\n\n",
    );
    writeFileSync(
      path.join(root, "work", "other-1.jsonl"),
      JSON.stringify(record) + "\n",
    );
    assert.deepEqual(readLedger('\uFEFF{"a":1}\r\n\r\n'), [{ a: 1 }]);
    assert.equal(ledgerRecords(root, "nc4").length, 1);
    assert.equal(path.basename(nextLedgerPath(root, "nc4")), "nc4-2.jsonl");
    assert.throws(() => ledgerRecords(root, "../x"), /batch-prefix/u);
    const live = path.join(root, "codex-live");
    mkdirSync(path.join(live, "s2-completed-expand"), { recursive: true });
    writeFileSync(
      path.join(live, "s2-completed-expand", "f0055-054631ms.png"),
      "",
    );
    writeFileSync(
      path.join(live, "s2-completed-expand", "f0056-054700ms.png"),
      "",
    );
    assert.deepEqual(
      liveFrames(live, "s2/f0055").map((file) => path.basename(file)),
      ["f0055-054631ms.png"],
    );
    assert.throws(() => liveFrames(live, "../f0055"), /batch-live-ref/u);
    const [pair] = pairRuns(root, ledgerRecords(root, "nc4"), live);
    assert.equal(path.basename(pair.png), "fork-dialog-codex-1888.png");
    assert.deepEqual(
      pair.live.map((file) => path.basename(file)),
      ["f0055-054631ms.png"],
    );
    assert.equal(pair.spec, "4");
    assert.deepEqual(pair.frames, []);
  } finally {
    rmSync(root, { recursive: true });
  }
});
