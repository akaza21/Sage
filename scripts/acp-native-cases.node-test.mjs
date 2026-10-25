import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { crc32, inflateSync } from "node:zlib";
import test from "node:test";
import { nativeCases } from "./acp-native-cases.mjs";
import { privacyFailuresForRecords } from "./check-harness-trace-privacy.mjs";
import { answer } from "./acp-native-case-events.mjs";
test("Codex child-opened admits child ownership before completion and keeps the reader alive", () => {
  const entry = nativeCases().find(({ id }) => id === "codex-child-opened");
  const ownerIndex = entry.steps.findIndex(
    (step) => step.params?.item?.type === "subAgentActivity",
  );
  assert.ok(ownerIndex >= 0, "child key alone does not admit a readable owner");
  const owner = entry.steps[ownerIndex];
  const spawnIndex = entry.steps.findIndex(
    (step) => step.params?.item?.type === "collabAgentToolCall",
  );
  assert.ok(spawnIndex >= 0, "retain the original collaboration observation");
  assert.equal(ownerIndex, spawnIndex + 1);
  const spawn = entry.steps[spawnIndex];
  assert.equal(spawn.method, "item/completed");
  assert.equal(spawn.params.threadId, "$SESSION");
  assert.equal(spawn.params.item.tool, "spawnAgent");
  assert.deepEqual(spawn.params.item.receiverThreadIds, ["fixture-child-1"]);
  assert.notEqual(owner.params.item.id, spawn.params.item.id);
  assert.equal(owner.method, "item/completed");
  assert.equal(owner.params.threadId, "$SESSION");
  assert.equal(owner.params.item.agentThreadId, "fixture-child-1");
  assert.ok(
    ownerIndex <
      entry.steps.findIndex((step) => step.shape === "codexCompleted"),
  );
  assert.ok(
    ownerIndex <
      entry.steps.findIndex((step) => step.kind === "codexUnsubscribe"),
  );
  assert.equal(entry.steps.at(-2).kind, "codexUnsubscribe");
});
test("native image wire payloads decode to an opaque, visible 320x180 synthetic PNG", () => {
  for (const harness of ["codex", "claude", "pi"]) {
    const entry = nativeCases().find(
      ({ id }) => id === `${harness}-answer-image`,
    );
    const frame = entry.steps.find((step) =>
      harness === "codex"
        ? step.params?.item?.type === "imageGeneration"
        : harness === "pi"
          ? step.frame?.type === "tool_execution_end"
          : step.update?.content?.type === "image",
    );
    const encoded =
      harness === "codex"
        ? frame.params.item.result.slice("data:image/png;base64,".length)
        : harness === "pi"
          ? frame.frame.result.content.find(({ type }) => type === "image").data
          : frame.update.content.data;
    const png = Buffer.from(encoded, "base64");
    assert.equal(png.subarray(0, 8).toString("hex"), "89504e470d0a1a0a");
    assert.equal(png.readUInt32BE(16), 320);
    assert.equal(png.readUInt32BE(20), 180);
    assert.equal(png[24], 8);
    assert.equal(png[25], 6);
    const compressed = [];
    let offset = 8;
    while (offset < png.length) {
      const length = png.readUInt32BE(offset);
      const chunk = png.subarray(offset + 4, offset + 8 + length);
      assert.equal(crc32(chunk), png.readUInt32BE(offset + 8 + length));
      if (chunk.subarray(0, 4).toString() === "IDAT")
        compressed.push(chunk.subarray(4));
      offset += length + 12;
    }
    assert.equal(offset, png.length);
    const pixels = inflateSync(Buffer.concat(compressed));
    assert.equal(pixels.length, 180 * (320 * 4 + 1));
    const colors = new Set();
    for (let row = 0; row < 180; row++) {
      const start = row * (320 * 4 + 1);
      assert.equal(pixels[start], 0);
      for (let column = 0; column < 320; column++) {
        const pixel = start + 1 + column * 4;
        assert.equal(pixels[pixel + 3], 255);
        colors.add(pixels.subarray(pixel, pixel + 3).toString("hex"));
      }
    }
    assert.ok(colors.size >= 3);
    assert.ok(png.length < 8192);
  }
});
test("jump fixture has a bounded transcript tall enough for every reference viewport", () => {
  const transcript = answer("jump-to-latest");
  assert.equal(transcript.match(/### Synthetic section /gu).length, 32);
  assert.ok(Buffer.byteLength(transcript) < 8192);
});
test("Claude approvals follow a populated setup turn and settle the intended second prompt", () => {
  for (const state of [
    "approval-pending",
    "approval-approved",
    "approval-declined",
  ]) {
    const { steps } = nativeCases().find(({ id }) => id === `claude-${state}`);
    const prompts = steps.flatMap((step, index) =>
      step.kind === "acpPrompt" ? [index] : [],
    );
    assert.equal(prompts.length, 2);
    assert.deepEqual(steps.slice(prompts[0] + 1, prompts[1]), [
      {
        op: "emitKnownFrame",
        shape: "acpUpdate",
        update: {
          sessionUpdate: "agent_message_chunk",
          content: {
            type: "text",
            text: "Synthetic approval setup completed. No tool was run.",
          },
        },
      },
      { op: "emitKnownFrame", shape: "acpCompleted" },
    ]);
    const permission = steps.findIndex(
      ({ shape }) => shape === "acpPermission",
    );
    const decision = steps.findIndex(
      ({ kind }) => kind === "acpPermissionDecision",
    );
    assert.ok(permission > prompts[1]);
    assert.equal(decision, permission + 1);
    const toolStates = steps
      .slice(prompts[1] + 1, decision)
      .flatMap((step) => (step.update?.toolCallId ? [step.update.status] : []));
    assert.deepEqual(toolStates, ["pending"]);
    assert.equal(
      steps.filter(({ shape }) => shape === "acpCompleted").length,
      2,
    );
    const afterDecision = steps
      .slice(decision + 1)
      .flatMap((step) => (step.update?.toolCallId ? [step.update.status] : []));
    assert.deepEqual(
      afterDecision,
      state === "approval-declined" ? ["failed"] : ["in_progress", "completed"],
    );
  }
});
test("fixture resource substitution joins root-relative path components natively", () => {
  const source = readFileSync(
    new URL("../src-tauri/src/acp/fixture/wire_acp.rs", import.meta.url),
    "utf8",
  );
  assert.match(source, /strip_prefix\("\$ROOT\/"\)/u);
  assert.match(source, /suffix\.split\('\/'\)/u);
});
test("stopped, failed and running fixtures never claim replay completion", () => {
  for (const state of [
    "turn-stopped",
    "turn-error",
    "turn-retry",
    "turn-running",
    "composer-stop",
  ]) {
    assert.equal(
      answer(state),
      "The synthetic example is available for native replay review.",
    );
    for (const harness of ["codex", "claude"]) {
      const entry = nativeCases().find(
        ({ id }) => id === `${harness}-${state}`,
      );
      assert.ok(entry);
      assert.doesNotMatch(
        JSON.stringify(entry.steps),
        /Native replay completed/u,
      );
    }
  }
});
test("native Codex cases hydrate Goal with the case's admitted model page", () => {
  for (const entry of nativeCases().filter(({ id }) =>
    id.startsWith("codex-"),
  )) {
    if (["codex-effort-menu", "codex-model-menu"].includes(entry.id)) {
      assert.deepEqual(entry.steps.slice(4, 9), [
        { op: "awaitClient", kind: "codexModelList" },
        { op: "emitKnownFrame", shape: "codexModelListEffortResult" },
        { op: "awaitClient", kind: "codexGoalGet" },
        { op: "emitKnownFrame", shape: "codexGoalMethodNotFound" },
        { op: "awaitClient", kind: "codexTurnStart" },
      ]);
      continue;
    }
    assert.deepEqual(
      entry.steps.slice(4, 7),
      [
        { op: "awaitClient", kind: "codexGoalGetWithOptionalModelList" },
        { op: "emitKnownFrame", shape: "codexGoalMethodNotFound" },
        { op: "awaitClient", kind: "codexTurnStart" },
      ],
      entry.id,
    );
  }
});

test("curated Codex journeys hydrate Models and Goal before the first turn", () => {
  const source = readFileSync(
    new URL("../src-tauri/src/acp/fixture/cases.rs", import.meta.url),
    "utf8",
  );
  const cases = [...source.matchAll(/r#"(\[.*?\])"#/gu)]
    .map((match) => JSON.parse(match[1]))
    .filter((steps) => steps.some(({ kind }) => kind === "codexTurnStart"));
  assert.equal(cases.length, 6);
  for (const steps of cases) {
    assert.deepEqual(steps.slice(4, 9), [
      { op: "awaitClient", kind: "codexModelList" },
      { op: "emitKnownFrame", shape: "codexModelListResult" },
      { op: "awaitClient", kind: "codexGoalGet" },
      { op: "emitKnownFrame", shape: "codexGoalMethodNotFound" },
      { op: "awaitClient", kind: "codexTurnStart" },
    ]);
  }
});

test("generated lost-Steer supports model-enabled and desktop admission", () => {
  const source = readFileSync(
    new URL(
      "../src-tauri/src/acp/fixture/generated_journey_cases.rs",
      import.meta.url,
    ),
    "utf8",
  );
  const match = source.match(
    /id: "codex-steer-lost-result", steps_json: r#"(\[.*?\])"#/u,
  );
  assert.ok(match);
  assert.deepEqual(JSON.parse(match[1]).slice(4, 7), [
    { op: "awaitClient", kind: "codexGoalGetWithOptionalModelList" },
    { op: "emitKnownFrame", shape: "codexGoalMethodNotFound" },
    { op: "awaitClient", kind: "codexTurnStart" },
  ]);
});
test("bounded native catalogue", () => {
  const cases = nativeCases();
  assert.ok(cases.length > 60 && cases.length < 240);
  assert.equal(new Set(cases.map(({ id }) => id)).size, cases.length);
  for (const entry of cases) {
    assert.match(entry.id, /^(codex|claude|pi)-[a-z0-9-]+$/u);
    assert.ok(entry.steps.length > 5 && entry.steps.length <= 64);
    assert.ok(Buffer.byteLength(JSON.stringify(entry.steps)) < 65536);
    assert.equal(entry.steps[0].op, "awaitClient");
    assert.ok(
      entry.steps.some(
        ({ kind }) =>
          kind === "acpPrompt" ||
          kind === "codexTurnStart" ||
          kind === "piPrompt",
      ),
    );
  }
  assert.deepEqual(privacyFailuresForRecords(cases, false), []);
  for (const state of [
    "turn-running",
    "turn-completed",
    "thinking-first-token",
    "tool-running",
    "files-pill",
    "plan-running",
    "turn-stopped",
    "answer-mermaid",
    "composer-stop",
  ]) {
    for (const harness of ["codex", "claude"])
      assert.ok(cases.some(({ id }) => id === harness + "-" + state));
  }
});

test("catalogue carries admitted wire evidence and finite continuations", () => {
  const catalogue = nativeCases();
  const steps = (id) => catalogue.find((entry) => entry.id === id).steps;
  const frames = (id) =>
    steps(id).filter((step) => step.op === "emitKnownFrame");
  for (const harness of ["codex", "claude"]) {
    assert.match(
      JSON.stringify(frames(harness + "-answer-code")),
      /export const example = 2/u,
    );
    assert.match(
      JSON.stringify(frames(harness + "-answer-mermaid")),
      /flowchart LR/u,
    );
    assert.match(
      JSON.stringify(frames(harness + "-answer-table-lists")),
      /Replay summary/u,
    );
    assert.equal(
      steps(harness + "-thinking-live").filter(
        (step) =>
          step.kind === (harness === "codex" ? "codexInterrupt" : "acpCancel"),
      ).length,
      1,
    );
    assert.equal(
      steps(harness + "-turn-retry").filter(
        (step) =>
          step.kind === (harness === "codex" ? "codexTurnStart" : "acpPrompt"),
      ).length,
      2,
    );
  }
  const codexEdit = frames("codex-file-edit-live").find(
    (step) => step.params?.item?.type === "fileChange",
  );
  assert.equal(codexEdit.method, "item/started");
  assert.equal(codexEdit.params.item.status, "inProgress");
  assert.match(codexEdit.params.item.changes[0].diff, /@@ -1/u);
  const claudeEdit = frames("claude-file-edit-live").find(
    (step) => step.update?.kind === "edit",
  );
  assert.equal(claudeEdit.update.content[0].type, "diff");
  assert.equal(claudeEdit.update.locations[0].path, "$ROOT/src/example.ts");
  const image = frames("codex-answer-image").find(
    (step) => step.params?.item?.type === "imageGeneration",
  );
  assert.match(image.params.item.result, /^data:image\/png;base64,/u);
  const delegates = frames("claude-subagents-grouped").filter(
    (step) => step.update?.sessionUpdate === "tool_call",
  );
  assert.equal(delegates.length, 2);
  for (const delegate of delegates)
    assert.deepEqual(delegate.update._meta, {
      claudeCode: { toolName: "Task" },
    });
  assert.equal(
    frames("claude-subagents-grouped").filter(
      (step) => step.update?.status === "completed",
    ).length,
    2,
  );
  for (const state of [
    "approval-pending",
    "approval-approved",
    "approval-declined",
  ]) {
    assert.equal(
      steps("claude-" + state).filter((step) => step.kind === "acpPrompt")
        .length,
      2,
    );
    assert.equal(
      steps("claude-" + state).filter(
        (step) => step.kind === "acpPermissionDecision",
      ).length,
      1,
    );
  }
  assert.equal(
    catalogue.some((entry) => entry.id === "claude-child-opened"),
    false,
  );
});

test("catalogue excludes states the binding cannot represent", () => {
  const ids = new Set(nativeCases().map(({ id }) => id));
  for (const id of [
    "codex-compaction-started",
    "codex-compaction-failed",
    "codex-compaction-cancelled",
    "claude-context-meter",
  ])
    assert.equal(ids.has(id), false, id);
  const completed = nativeCases().find(
    ({ id }) => id === "codex-compaction-completed",
  );
  const compaction = completed.steps.filter(
    (step) => step.params?.item?.type === "contextCompaction",
  );
  assert.equal(compaction.length, 1);
  assert.equal(compaction[0].method, "item/completed");
});

test("Codex context meter carries the full pinned token usage shape", () => {
  const entry = nativeCases().find(({ id }) => id === "codex-context-meter");
  const usage = entry.steps.find(
    (step) => step.method === "thread/tokenUsage/updated",
  );
  assert.ok(usage);
  assert.deepEqual(usage.params.tokenUsage, {
    last: {
      cachedInputTokens: 512,
      inputTokens: 4096,
      outputTokens: 1024,
      reasoningOutputTokens: 256,
      totalTokens: 5120,
    },
    total: {
      cachedInputTokens: 512,
      inputTokens: 4096,
      outputTokens: 1024,
      reasoningOutputTokens: 256,
      totalTokens: 5120,
    },
    modelContextWindow: 32768,
  });
  const evidence = JSON.parse(
    readFileSync(
      new URL(
        "../src-tauri/testdata/acp/codex/native-capture-wire-0146.json",
        import.meta.url,
      ),
      "utf8",
    ),
  );
  assert.equal(evidence.runtimeVersion, "0.146.0");
  assert.equal(
    evidence.appServerSchemaSha256,
    "d3992fec1398afdbec658da2c720c6993fbf3c1ce4900785694d2196679eddfc",
  );
  for (const field of evidence.threadTokenUsageRequired)
    assert.ok(field in usage.params.tokenUsage);
  for (const field of evidence.usageBreakdownRequired) {
    assert.ok(field in usage.params.tokenUsage.last);
    assert.ok(field in usage.params.tokenUsage.total);
  }
});
