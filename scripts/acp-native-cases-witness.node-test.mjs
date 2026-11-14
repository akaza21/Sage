import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync, mkdtempSync, rmSync } from "node:fs";
import path from "node:path";
import { tmpdir } from "node:os";
import { nativeCases } from "./acp-native-cases.mjs";
import { freshRoots } from "./capture-native-chat-process.mjs";

test("Claude approval witnesses match 0.76.0 Bash permission presentation", () => {
  for (const state of [
    "approval-pending",
    "approval-approved",
    "approval-declined",
  ]) {
    const { steps } = nativeCases().find(({ id }) => id === `claude-${state}`);
    const permission = steps.find(({ shape }) => shape === "acpPermission");
    assert.deepEqual(permission.toolCall, {
      toolCallId: "fixture-tool-1",
      name: "Bash",
      status: "pending",
      rawInput: {
        command: String.raw`printf '%s\n' 'Inspect synthetic example 1'`,
      },
      title: "Bash",
      kind: "execute",
      content: [],
    });
    assert.deepEqual(permission._meta, {
      permission: { version: 1, title: "Bash" },
    });
    const opening = steps.find(
      ({ update }) => update?.sessionUpdate === "tool_call",
    ).update;
    assert.equal(opening.title, permission.toolCall.rawInput.command);
    assert.equal(opening.status, "pending");
    assert.deepEqual(opening._meta, { claudeCode: { toolName: "Bash" } });
  }
});

test("Claude command witnesses carry Bash input and adapter console output on one tool identity", () => {
  for (const state of [
    "tool-running",
    "tool-finished",
    "tool-expanded",
    "tools-grouped",
    "worked-expanded",
    "git-running",
    "git-finished",
    "tool-long-label",
  ]) {
    const { steps } = nativeCases().find(({ id }) => id === `claude-${state}`);
    const calls = steps.filter(
      ({ update }) => update?.sessionUpdate === "tool_call",
    );
    assert.equal(
      calls.length,
      ["tools-grouped", "worked-expanded"].includes(state) ? 3 : 1,
    );
    for (const [index, { update }] of calls.entries()) {
      assert.equal(update.kind, "execute");
      assert.equal(update.status, "pending");
      assert.deepEqual(update._meta, { claudeCode: { toolName: "Bash" } });
      assert.equal(update.rawInput.command, update.title);
      assert.match(update.rawInput.command, /^(git status --short|printf )/u);
      if (state !== "tool-long-label")
        assert.equal(
          update.rawInput.command,
          state.startsWith("git")
            ? "git status --short"
            : String.raw`printf '%s\n' 'Inspect synthetic example ${index + 1}'`,
        );
      assert.deepEqual(update.content, []);
      const updates = steps.filter(
        (step) =>
          step.update?.sessionUpdate === "tool_call_update" &&
          step.update.toolCallId === update.toolCallId,
      );
      assert.equal(updates[0].update.status, "in_progress");
      assert.deepEqual(updates[0].update._meta, {
        claudeCode: {
          toolName: "Bash",
          toolResponse: { elapsedTimeSeconds: 1 },
        },
      });
      if (state.endsWith("running")) {
        assert.equal(updates.length, 1);
        assert.equal(updates[0].update.rawOutput, undefined);
      } else {
        assert.equal(updates.length, 2);
        assert.equal(updates[1].update.status, "completed");
        assert.equal(
          updates[1].update.rawOutput,
          "Synthetic tool finished; no command was executed.",
        );
        assert.deepEqual(updates[1].update.content, [
          {
            type: "content",
            content: {
              type: "text",
              text: "```console\nSynthetic tool finished; no command was executed.\n```",
            },
          },
        ]);
        assert.equal(updates[1].update.rawInput, undefined);
      }
    }
  }
});
test("Claude approval decisions retain real Bash progress and success or error output shapes", () => {
  for (const state of ["approval-approved", "approval-declined"]) {
    const { steps } = nativeCases().find(({ id }) => id === `claude-${state}`);
    const updates = steps.filter(
      ({ update }) => update?.sessionUpdate === "tool_call_update",
    );
    const declined = state === "approval-declined";
    assert.equal(updates.length, declined ? 1 : 2);
    if (!declined)
      assert.deepEqual(updates[0].update, {
        sessionUpdate: "tool_call_update",
        toolCallId: "fixture-tool-1",
        status: "in_progress",
        _meta: {
          claudeCode: {
            toolName: "Bash",
            toolResponse: { elapsedTimeSeconds: 1 },
          },
        },
      });
    const output = declined
      ? "Synthetic approval declined; no command was executed."
      : "Synthetic tool finished; no command was executed.";
    assert.deepEqual(updates.at(-1).update, {
      sessionUpdate: "tool_call_update",
      toolCallId: "fixture-tool-1",
      status: declined ? "failed" : "completed",
      _meta: { claudeCode: { toolName: "Bash" } },
      rawOutput: output,
      content: [
        {
          type: "content",
          content: {
            type: "text",
            text: `${declined ? "```" : "```console"}\n${output}\n\`\`\``,
          },
        },
      ],
    });
  }
});
test("Codex model and effort menu witnesses negotiate a terminal model page before Goal and prompt", () => {
  for (const state of ["model-menu", "effort-menu"]) {
    const { steps } = nativeCases().find(({ id }) => id === `codex-${state}`);
    assert.deepEqual(steps.slice(2, 4), [
      { op: "awaitClient", kind: "codexStart" },
      { op: "emitKnownFrame", shape: "codexThreadEffortResult" },
    ]);
    assert.deepEqual(steps.slice(4, 9), [
      { op: "awaitClient", kind: "codexModelList" },
      { op: "emitKnownFrame", shape: "codexModelListEffortResult" },
      { op: "awaitClient", kind: "codexGoalGet" },
      { op: "emitKnownFrame", shape: "codexGoalMethodNotFound" },
      { op: "awaitClient", kind: "codexTurnStart" },
    ]);
  }
});
test("native file witnesses resolve to the existing file provisioned for Preview and Open diff", () => {
  const workspace = mkdtempSync(path.join(tmpdir(), "sage-native-witness-"));
  try {
    const { project } = freshRoots(workspace);
    const imageCase = nativeCases().find(({ id }) => id === "pi-answer-image");
    const imageStart = imageCase.steps.find(
      ({ frame }) => frame?.type === "tool_execution_start",
    );
    const imageEnd = imageCase.steps.find(
      ({ frame }) => frame?.type === "tool_execution_end",
    );
    const imageFile = path.join(
      project,
      ...imageStart.frame.args.path.slice("$ROOT/".length).split("/"),
    );
    assert.deepEqual(
      readFileSync(imageFile),
      Buffer.from(
        imageEnd.frame.result.content.find(({ type }) => type === "image").data,
        "base64",
      ),
    );
    for (const harness of ["claude", "codex"]) {
      const { steps } = nativeCases().find(
        ({ id }) => id === `${harness}-file-edit-live`,
      );
      const edit = steps.find(
        (step) =>
          step.update?.kind === "edit" ||
          step.params?.item?.type === "fileChange",
      );
      const wirePath =
        harness === "claude"
          ? edit.update.content[0].path
          : edit.params.item.changes[0].path;
      const target = path.join(
        project,
        ...wirePath.slice("$ROOT/".length).split("/"),
      );
      assert.equal(readFileSync(target, "utf8"), "export const example = 1;\n");
    }
  } finally {
    rmSync(workspace, { recursive: true });
  }
});
test("every native generated wire sequence equals its source catalogue", () => {
  // Journey cases are generated into the native-case catalogue module.
  const source = readFileSync(
    new URL(
      "../crates/sage-acp-protocol/src/native_cases/generated_journey_cases.rs",
      import.meta.url,
    ),
    "utf8",
  );
  const generated = new Map(
    [
      ...source.matchAll(
        /JourneyCase \{ id: "([^"]+)", steps_json: r#"(\[.*?\])"#/gu,
      ),
    ].map((match) => [match[1], JSON.parse(match[2])]),
  );
  for (const { id, steps } of nativeCases())
    assert.equal(JSON.stringify(generated.get(id)), JSON.stringify(steps), id);
});
test("live file witnesses start an edit and wait for cancellation without completing it", () => {
  for (const harness of ["codex", "claude"]) {
    for (const state of [
      "file-edit-live",
      "files-pill",
      "file-hunk",
      "file-diff-tab",
    ]) {
      const { steps } = nativeCases().find(
        ({ id }) => id === `${harness}-${state}`,
      );
      const edits = steps.filter(
        (step) =>
          step.params?.item?.type === "fileChange" ||
          step.update?.kind === "edit",
      );
      const live = state === "file-edit-live";
      assert.equal(edits.length, live ? 1 : 2);
      if (harness === "codex") {
        assert.equal(edits[0].method, "item/started");
        assert.equal(edits[0].params.item.status, "inProgress");
        assert.deepEqual(edits[0].params.item.changes, [
          {
            path: "$ROOT/src/example.ts",
            kind: { type: "update" },
            diff: "@@ -1 +1 @@\n-export const example = 1;\n+export const example = 2;\n",
          },
        ]);
        if (!live) {
          assert.equal(edits[1].method, "item/completed");
          assert.equal(edits[1].params.item.id, edits[0].params.item.id);
          assert.equal(edits[1].params.item.status, "completed");
        }
      } else {
        assert.equal(edits[0].update.sessionUpdate, "tool_call");
        assert.equal(edits[0].update.status, "pending");
        const progress = steps.find(
          ({ update }) =>
            update?.toolCallId === "fixture-edit" &&
            update.status === "in_progress",
        );
        assert.deepEqual(progress?.update, {
          sessionUpdate: "tool_call_update",
          toolCallId: "fixture-edit",
          status: "in_progress",
          _meta: {
            claudeCode: {
              toolName: "Edit",
              toolResponse: { elapsedTimeSeconds: 1 },
            },
          },
        });
        assert.deepEqual(edits[0].update.content, [
          {
            type: "diff",
            path: "$ROOT/src/example.ts",
            oldText: "export const example = 1;\n",
            newText: "export const example = 2;\n",
          },
        ]);
        if (!live) {
          assert.equal(edits[1].update.sessionUpdate, "tool_call_update");
          assert.equal(edits[1].update.toolCallId, edits[0].update.toolCallId);
          assert.equal(edits[1].update.status, "completed");
        }
      }
      const cancel = steps.findIndex(
        ({ kind }) =>
          kind === (harness === "codex" ? "codexInterrupt" : "acpCancel"),
      );
      assert.equal(cancel >= 0, live);
      if (live) assert.ok(cancel > steps.indexOf(edits[0]));
    }
  }
});
