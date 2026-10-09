export const NATIVE_IMAGE_BASE64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg==";
export const emit = (shape, extra = {}) => ({
  op: "emitKnownFrame",
  shape,
  ...extra,
});
export const wait = (kind) => ({ op: "awaitClient", kind });

const ROOT = "$ROOT/src/example.ts";
const longLabel =
  "Inspect the bounded synthetic workspace and compare deterministic native replay presentation without executing commands";
const answer = (state) => {
  if (state === "answer-table-lists")
    return "## Replay summary\n\n| State | Result |\n| --- | --- |\n| Native binding | Verified replay |\n| Storage | Durable projection |\n\n- Fixed synthetic input\n- Bounded protocol events\n\n1. Inspect\n2. Compare";
  if (state === "answer-code")
    return "Synthetic implementation:\n\n```ts\nexport const example = 2;\n```";
  if (state === "answer-mermaid")
    return "```mermaid\nflowchart LR\n  Binding --> Pump\n  Pump --> SQLite\n```";
  if (state === "jump-to-latest")
    return Array.from(
      { length: 16 },
      (_, index) =>
        "### Synthetic section " +
        (index + 1) +
        "\n\nA bounded replay paragraph verifies native scrolling and transcript layout.",
    ).join("\n\n");
  return "Native replay completed. The synthetic example is ready for review.";
};

export function events(harness, state) {
  const frames = [];
  const acp = (update) => frames.push(emit("acpUpdate", { update }));
  const codex = (method, params) =>
    frames.push(emit("codexNative", { method, params }));
  const text = (value, thought = false) =>
    harness === "claude"
      ? acp({
          sessionUpdate: thought
            ? "agent_thought_chunk"
            : "agent_message_chunk",
          content: { type: "text", text: value },
        })
      : codex(
          thought
            ? "item/reasoning/summaryTextDelta"
            : "item/agentMessage/delta",
          {
            itemId: thought ? "fixture-thinking" : "fixture-answer",
            summaryIndex: 0,
            delta: value,
          },
        );
  const tool = (index, kind, title, finished, extra = {}) => {
    const id = "fixture-tool-" + index;
    if (harness === "claude") {
      acp({
        sessionUpdate: "tool_call",
        toolCallId: id,
        kind,
        title,
        status: "in_progress",
        ...extra,
      });
      if (finished)
        acp({
          sessionUpdate: "tool_call_update",
          toolCallId: id,
          kind,
          status: "completed",
          content: [
            {
              type: "content",
              content: {
                type: "text",
                text: "Synthetic tool finished; no command was executed.",
              },
            },
          ],
          ...extra,
        });
    } else {
      const item = {
        type: "commandExecution",
        id,
        command: title,
        status: "inProgress",
        cwd: "$ROOT",
        commandActions: [],
        ...extra,
      };
      codex("item/started", { item });
      if (finished)
        codex("item/completed", {
          item: {
            ...item,
            status: "completed",
            aggregatedOutput:
              "Synthetic tool finished; no command was executed.",
            exitCode: 0,
          },
        });
    }
  };
  if (state === "prose-then-thinking")
    text("I will inspect the synthetic example first.");
  if (state.includes("thinking")) {
    text("Checking the native binding", true);
    if (state !== "thinking-first-token")
      text(" and durable projection before making a bounded change.", true);
  } else if (/^(tool|tools|git|worked)/u.test(state)) {
    const finished = !state.endsWith("running");
    const count =
      state === "tools-grouped" || state === "worked-expanded" ? 3 : 1;
    for (let index = 1; index <= count; index += 1)
      tool(
        index,
        "execute",
        state === "tool-long-label"
          ? longLabel
          : state.startsWith("git")
            ? "git status --short (synthetic observation)"
            : "Inspect synthetic example " + index,
        finished,
      );
  } else if (/^(file|files)/u.test(state)) {
    if (harness === "claude") {
      const locations = [{ path: ROOT, line: 1 }];
      acp({
        sessionUpdate: "tool_call",
        toolCallId: "fixture-edit",
        kind: "edit",
        title: "Update synthetic example",
        status: "in_progress",
        locations,
        content: [
          {
            type: "diff",
            path: ROOT,
            oldText: "export const example = 1;\n",
            newText: "export const example = 2;\n",
          },
        ],
      });
      if (state !== "file-edit-live")
        acp({
          sessionUpdate: "tool_call_update",
          toolCallId: "fixture-edit",
          kind: "edit",
          status: "completed",
          locations,
          content: [
            {
              type: "diff",
              path: ROOT,
              oldText: "export const example = 1;\n",
              newText: "export const example = 2;\n",
            },
          ],
        });
    } else {
      const item = {
        type: "fileChange",
        id: "fixture-edit",
        status: "completed",
        changes: [
          {
            path: ROOT,
            kind: { type: "update" },
            diff: "@@ -1 +1 @@\n-export const example = 1;\n+export const example = 2;\n",
          },
        ],
      };
      codex("item/completed", {
        item,
      });
    }
  } else if (state.startsWith("subagents") || state === "child-opened") {
    if (harness === "claude") {
      const count = state === "subagents-grouped" ? 2 : 1;
      for (let index = 1; index <= count; index += 1)
        tool(
          index,
          "other",
          "Synthetic bounded delegate metadata",
          state !== "subagents-started",
          { _meta: { claudeCode: { toolName: "Task" } } },
        );
    } else {
      const count = state === "subagents-grouped" ? 2 : 1;
      for (let index = 1; index <= count; index += 1) {
        const finished = state !== "subagents-started";
        const child = "fixture-child-" + index;
        const item = {
          type: "collabAgentToolCall",
          id: "fixture-delegate-" + index,
          tool: "spawnAgent",
          status: finished ? "completed" : "inProgress",
          senderThreadId: "$SESSION",
          receiverThreadIds: [child],
          agentsStates: {
            [child]: {
              status: finished ? "completed" : "running",
              message: "Synthetic child summary",
            },
          },
        };
        codex(finished ? "item/completed" : "item/started", { item });
      }
    }
  } else if (state.startsWith("plan")) {
    const entries = [
      {
        content: "Inspect the synthetic example",
        status: state === "plan-completed" ? "completed" : "in_progress",
        priority: "high",
      },
      {
        content: "Verify native replay",
        status: state === "plan-completed" ? "completed" : "pending",
        priority: "medium",
      },
    ];
    if (harness === "claude") acp({ sessionUpdate: "plan", entries });
    else
      codex("turn/plan/updated", {
        explanation: "Bounded synthetic plan",
        plan: entries.map(({ content, status }) => ({
          step: content,
          status: status === "in_progress" ? "inProgress" : status,
        })),
      });
  } else if (state === "compaction-completed") {
    codex("item/completed", {
      item: { type: "contextCompaction", id: "fixture-compaction" },
    });
  } else if (state.startsWith("approval")) {
    tool(1, "execute", "Inspect synthetic example", false);
    frames.push(emit("acpPermission"));
    frames.push(wait("acpPermissionDecision"));
    acp({
      sessionUpdate: "tool_call_update",
      toolCallId: "fixture-tool-1",
      kind: "execute",
      status: state === "approval-declined" ? "failed" : "completed",
    });
  } else if (state === "context-meter") {
    const usage = {
      cachedInputTokens: 512,
      inputTokens: 4096,
      outputTokens: 1024,
      reasoningOutputTokens: 256,
      totalTokens: 5120,
    };
    codex("thread/tokenUsage/updated", {
      tokenUsage: { last: usage, total: usage, modelContextWindow: 32768 },
    });
    text("Synthetic context usage is available.");
  } else if (state === "answer-image") {
    if (harness === "claude")
      acp({
        sessionUpdate: "agent_message_chunk",
        content: {
          type: "image",
          mimeType: "image/png",
          data: NATIVE_IMAGE_BASE64,
        },
      });
    else
      codex("item/completed", {
        item: {
          type: "imageGeneration",
          id: "fixture-image",
          result: "data:image/png;base64," + NATIVE_IMAGE_BASE64,
        },
      });
  } else {
    const value = "Synthetic replay.\n\n" + answer(state);
    text(value);
  }
  return frames;
}
