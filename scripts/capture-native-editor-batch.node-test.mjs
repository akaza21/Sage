import assert from "node:assert/strict";
import test from "node:test";
import {
  planEditorRuns,
  parseEditorRun,
  runEditorBatch,
} from "./capture-native-editor-batch.mjs";
import { nextBatch } from "./capture-native-chat-batch.mjs";
import { selectOwnedTarget } from "./capture-native-chat-contract.mjs";
import { PACKAGED_PAGE } from "./capture-native-editor-capture.mjs";

test("Editor plan contains only the seven supported cells, never absence stand-ins", () => {
  assert.deepEqual(planEditorRuns(), [
    "editor:editor-idle:1888",
    "editor:outline:1888",
    "editor:outline:720",
    "editor:outline:480",
    "editor:go-to-line:1888",
    "editor:search-view:1888",
    "editor:settings-editor:1888",
  ]);
  assert.throws(
    () => parseEditorRun("editor:quick-open:480"),
    /truthful-absent-state/u,
  );
  assert.throws(() => parseEditorRun("codex:editor-idle:1888"), /batch-run/u);
});
test("shared batch helper writes the chat ledger shape, sequentially and bounded", async () => {
  const appended = [],
    captured = [];
  const result = await runEditorBatch({
    runs: planEditorRuns().slice(0, 2),
    check: async () => null,
    capture: async (run) => {
      captured.push(run);
      return {
        exit: 0,
        result: {
          verdict: "captured",
          run: "owned-run",
          directory: "work/native-captures/owned-run",
        },
      };
    },
    append: (record) => appended.push(record),
    now: () => 1000,
  });
  assert.equal(result.aborted, null);
  assert.equal(appended.length, 2);
  assert.deepEqual(Object.keys(appended[0]), [
    "harness",
    "state",
    "width",
    "exit",
    "secs",
    "result",
  ]);
  assert.deepEqual(captured[1], {
    harness: "editor",
    state: "outline",
    width: 1888,
  });
  await assert.rejects(
    () => runEditorBatch({ runs: Array(31).fill("editor:outline:1888") }),
    /batch-size/u,
  );
});
test("preflight blocker does not launch Editor, and same-build probes resume", async () => {
  let launched = false;
  const result = await runEditorBatch({
    runs: planEditorRuns(),
    check: async () => "tree dirty",
    capture: async () => {
      launched = true;
    },
    append: () => {},
  });
  assert.match(result.aborted, /tree dirty/u);
  assert.equal(launched, false);
  const build = "a".repeat(64);
  const order = planEditorRuns();
  assert.deepEqual(
    nextBatch({
      order,
      build,
      records: [
        { ...parseEditorRun(order[0]), result: { verdict: "captured" } },
      ],
      probeFor: () => ({ verdict: "captured", hashes: { build } }),
    }),
    order.slice(1),
  );
});
test("owned packaged target is explicit, not a dev-server or arbitrary browser tab", () => {
  const port = 19567;
  const target = {
    id: "sage-page",
    type: "page",
    title: "Sage",
    url: PACKAGED_PAGE,
    webSocketDebuggerUrl: `ws://127.0.0.1:${port}/devtools/page/sage-page`,
  };
  assert.equal(selectOwnedTarget([target], port, PACKAGED_PAGE), target);
  assert.equal(
    selectOwnedTarget(
      [{ ...target, url: "https://example.com/" }],
      port,
      PACKAGED_PAGE,
    ),
    null,
  );
  assert.throws(
    () => selectOwnedTarget([target, target], port, PACKAGED_PAGE),
    /cdp-target/u,
  );
});
