import assert from "node:assert/strict";
import test from "node:test";
import { nativeCases } from "./acp-native-cases.mjs";
import { privacyFailuresForRecords } from "./check-harness-trace-privacy.mjs";
test("bounded native catalogue", () => {
  const cases = nativeCases();
  assert.ok(cases.length > 60 && cases.length < 160);
  assert.equal(new Set(cases.map(({ id }) => id)).size, cases.length);
  for (const entry of cases) {
    assert.match(entry.id, /^(codex|claude)-[a-z0-9-]+$/u);
    assert.ok(entry.steps.length > 5 && entry.steps.length <= 64);
    assert.ok(Buffer.byteLength(JSON.stringify(entry.steps)) < 65536);
    assert.equal(entry.steps[0].op, "awaitClient");
    assert.ok(
      entry.steps.some(
        ({ kind }) => kind === "acpPrompt" || kind === "codexTurnStart",
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
  assert.equal(codexEdit.method, "item/completed");
  assert.equal(codexEdit.params.item.status, "completed");
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
    assert.equal(delegate.update._meta.toolName, "Task");
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
