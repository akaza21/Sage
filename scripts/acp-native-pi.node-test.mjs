import assert from "node:assert/strict";
import test from "node:test";
import { nativeCases, truthfulAbsentStates } from "./acp-native-cases.mjs";
import { parseArgs } from "./capture-native-chat-contract.mjs";
import { privacyFailuresForRecords } from "./check-harness-trace-privacy.mjs";
import { readFileSync } from "node:fs";
import {
  actionsFor,
  namedState,
  scopeToTurn,
} from "./capture-native-chat-dom.mjs";
import { piIdleConfigJourney } from "./acp-native-pi-cases.mjs";
import { freshRoots } from "./capture-native-chat-process.mjs";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

test("Pi grouped read paths exist in the disposable native replay project", () => {
  const workspace = mkdtempSync(path.join(tmpdir(), "sage-activity-parity-"));
  try {
    const roots = freshRoots(workspace);
    const reads = nativeCases()
      .find(({ id }) => id === "pi-tools-grouped")
      .steps.filter(({ frame }) => frame?.type === "tool_execution_start");
    for (const { frame } of reads) {
      const file = frame.args.path.replace("$ROOT", roots.project);
      assert.equal(readFileSync(file, "utf8"), "export const example = 1;\n");
    }
  } finally {
    rmSync(workspace, { recursive: true, force: true });
  }
});

test("Pi grouped reads use three distinct files while tool-finished stays a singleton", () => {
  const calls = (state) =>
    nativeCases()
      .find(({ id }) => id === `pi-${state}`)
      .steps.filter(({ frame }) => frame?.type === "tool_execution_start")
      .map(({ frame }) => frame.args.path);
  assert.deepEqual(calls("tool-finished"), ["$ROOT/src/example.ts"]);
  assert.deepEqual(calls("tools-grouped"), [
    "$ROOT/src/example-1.ts",
    "$ROOT/src/example-2.ts",
    "$ROOT/src/example-3.ts",
  ]);
});

test("Pi curated catalogue derives transport from the same registry that admits its fixture", () => {
  const source = readFileSync(
    new URL("../src-tauri/src/harnesses.rs", import.meta.url),
    "utf8",
  );
  const metadata = source.slice(
    source.indexOf('"pi",\n                "Pi",'),
    source.indexOf('"gemini-cli",\n                "Gemini CLI",'),
  );
  assert.match(metadata, /self\.registered_protocol\("pi"\)/u);
  assert.doesNotMatch(metadata, /CuratedProtocol::acp\(None\)/u);
});

test("Pi streams deltas only and terminal messages match the assembled deltas", () => {
  const cases = [
    ...nativeCases().filter(({ id }) => id.startsWith("pi-")),
    piIdleConfigJourney(),
  ];
  for (const entry of cases) {
    let content = [];
    for (const { frame } of entry.steps) {
      if (frame?.type === "message_start") content = [];
      if (frame?.type === "message_update") {
        const event = frame.assistantMessageEvent;
        const type = event.type.startsWith("thinking") ? "thinking" : "text";
        const index = event.contentIndex;
        if (event.type.endsWith("start")) content[index] = { type, [type]: "" };
        else if (event.type.endsWith("delta")) {
          content[index] ??= { type, [type]: "" };
          assert.equal(content[index].type, type, entry.id);
          content[index][type] += event.delta;
        }
        assert.equal(frame.message, undefined, entry.id);
        assert.equal(event.partial, undefined, entry.id);
      }
      if (["message_end", "turn_end"].includes(frame?.type))
        assert.deepEqual(frame.message.content, content, entry.id);
      if (frame?.type === "agent_end")
        assert.deepEqual(frame.messages.at(-1).content, content, entry.id);
    }
  }
});

test("Pi capture admits source-shaped read-only replay and refuses absent states", () => {
  const cases = nativeCases().filter(({ id }) => id.startsWith("pi-"));
  for (const state of [
    "turn-running",
    "turn-completed",
    "thinking-first-token",
    "thinking-live",
    "tool-running",
    "tool-finished",
    "tools-grouped",
    "tool-expanded",
    "compaction-started",
    "compaction-completed",
    "compaction-failed",
    "compaction-cancelled",
    "turn-error",
    "turn-stopped",
    "turn-retry",
    "answer-table-lists",
    "answer-code",
    "answer-mermaid",
    "answer-image",
    "composer-idle",
    "composer-stop",
    "model-menu",
    "effort-menu",
    "add-menu",
    "access-menu",
    "non-read-denial",
    "cancelled-dialog",
  ]) {
    const entry = cases.find(({ id }) => id === "pi-" + state);
    assert.ok(entry, state);
    assert.equal(
      parseArgs(["--harness", "pi", "--state", state, "--width", "1888"])
        .caseId,
      entry.id,
    );
    assert.deepEqual(entry.steps.slice(0, 5), [
      { op: "awaitClient", kind: "piInitialize" },
      { op: "awaitClient", kind: "piModels" },
      { op: "awaitClient", kind: "piNew" },
      { op: "awaitClient", kind: "piPrompt" },
      { op: "awaitClient", kind: "piThinkingLevels" },
    ]);
    assert.ok(
      entry.steps.some(({ frame }) => frame?.type === "agent_settled"),
      state,
    );
    assert.ok(entry.steps.length <= 64, state);
  }
  assert.deepEqual(privacyFailuresForRecords(cases, false), []);
  for (const state of truthfulAbsentStates.pi)
    assert.throws(
      () => parseArgs(["--harness", "pi", "--state", state, "--width", "720"]),
      /truthful-absent-state/u,
    );
});

test("Pi compaction waits through agent_end and replays continuation before settlement", () => {
  const evidence = JSON.parse(
    readFileSync(
      new URL(
        "../src-tauri/testdata/acp/pi/native-capture-wire-150.json",
        import.meta.url,
      ),
      "utf8",
    ),
  );
  assert.equal(
    evidence.sourceCommit,
    "f9a26365568127461c47a07a37dcb42c140a3ac5",
  );
  const cases = nativeCases().filter(({ id }) => id.startsWith("pi-"));
  for (const entry of cases) {
    for (const { frame } of entry.steps) {
      if (!frame) continue;
      const required =
        frame.type === "message_update"
          ? evidence.messageUpdateRequired
          : frame.type === "tool_execution_start"
            ? evidence.toolStartRequired
            : frame.type === "tool_execution_end"
              ? evidence.toolEndRequired
              : [];
      for (const field of required)
        assert.ok(field in frame, entry.id + ":" + field);
    }
  }
  const steps = cases.find(({ id }) => id === "pi-compaction-started").steps;
  const end = steps.findIndex(({ frame }) => frame?.type === "agent_end");
  const compaction = steps.findIndex(
    ({ frame }) => frame?.type === "compaction_start",
  );
  const abort = steps.findIndex(({ kind }) => kind === "piAbort");
  const settled = steps.findIndex(
    ({ frame }) => frame?.type === evidence.settlementEvent,
  );
  assert.ok(end < compaction && compaction < abort && abort < settled);
  const completed = cases.find(
    ({ id }) => id === "pi-compaction-completed",
  ).steps;
  assert.equal(
    completed.filter(({ frame }) => frame?.type === "agent_start").length,
    2,
  );
  assert.equal(
    completed.filter(({ frame }) => frame?.type === "agent_settled").length,
    1,
  );
});

test("Pi capture predicates distinguish compaction outcomes and read-only notices", () => {
  assert.deepEqual(
    actionsFor("cancelled-dialog", "pi").map((action) =>
      scopeToTurn(action, "pi-turn"),
    ),
    // The cancelled-dialog notice is a plain lifecycle status line, not a
    // disclosure, so only the worked group opens.
    ['.acp-turn[data-turn-id="pi-turn"] .chat-transcript-work > summary'],
  );
  const observation = {
    bindingId: "pi-rpc",
    selected: "pi",
    state: "completed",
    visible: { compaction: true, notice: true, tool: true },
    itemKinds: [],
    controls: {},
    toolStates: ["completed"],
    noticeKinds: ["pi_dialog_cancelled"],
    compactionStates: ["failed"],
    assistantRendered: true,
    lazyFallbackVisible: false,
  };
  const check = (state, changes = {}) =>
    namedState({ harness: "pi", state }, { ...observation, ...changes })
      .namedState;
  assert.equal(check("compaction-failed"), true);
  assert.equal(check("compaction-completed"), false);
  assert.equal(check("compaction-cancelled"), false);
  assert.equal(
    check("compaction-started", {
      state: "running",
      compactionStates: ["started"],
    }),
    true,
  );
  assert.equal(
    check("compaction-started", {
      state: "completed",
      compactionStates: ["started"],
    }),
    false,
  );
  assert.equal(check("cancelled-dialog"), true);
  assert.equal(check("cancelled-dialog", { noticeKinds: ["other"] }), false);
  assert.equal(
    check("non-read-denial", {
      state: "interrupted",
      toolStates: ["declined"],
    }),
    true,
  );
  assert.equal(check("non-read-denial", { state: "interrupted" }), false);
});

test("Pi retry continuation keeps one prompt and settles only after recovery", () => {
  const steps = nativeCases().find(({ id }) => id === "pi-turn-retry").steps;
  assert.equal(steps.filter(({ kind }) => kind === "piPrompt").length, 1);
  assert.equal(
    steps.filter(({ frame }) => frame?.type === "agent_settled").length,
    1,
  );
  const retry = steps.findIndex(
    ({ frame }) => frame?.type === "auto_retry_start",
  );
  const recovered = steps.findIndex(
    ({ frame }) => frame?.type === "auto_retry_end" && frame.success === true,
  );
  const settled = steps.findIndex(
    ({ frame }) => frame?.type === "agent_settled",
  );
  assert.ok(retry < recovered && recovered < settled);
  assert.deepEqual(actionsFor("turn-retry", "pi"), []);
  const observation = {
    state: "completed",
    visible: { answer: true },
    itemKinds: ["text"],
    controls: {},
    turnCount: 1,
    failedTurns: 0,
    elements: [{ kind: "answer", text: "Synthetic retry completed." }],
    assistantRendered: true,
    lazyFallbackVisible: false,
  };
  assert.equal(
    namedState({ harness: "pi", state: "turn-retry" }, observation).namedState,
    true,
  );
});
