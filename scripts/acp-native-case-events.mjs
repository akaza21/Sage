export const NATIVE_IMAGE_BASE64 =
  "iVBORw0KGgoAAAANSUhEUgAAAUAAAAC0CAYAAADl5PURAAADa0lEQVR42u3dsRGCUBRFQUqxCisgtQcbMKNBKqACAyv5ZMwQmolng23gzb8nhOl2fwyAoskRAAEEEEAAAQQQQAABBBBAAAEEEEAAAQQQQAABBBBAAAEEEEAAAQQQQAABBBBAAAEEEEAAAQROxvb8mgACqeiJoQCC8MVDKIAgfNkQCiCIXzaCAgjil42gAIL4ZSMogCB+2QgKIIhfNoICCOKXjaAAgvhlIyiAIIACCIhfLYICCAIogID41SIogCCAAggIoAAC4heJoACCAAogIIACCAigAAICKICAAAogIIACCAigAAICKICAAAogIIACCAigAAICKICAr8EIICCAAggIoAACV4zg1W8tgDHr+3NwDwEUQA8uGT8RFEH/BRbAdPxEUAAF0GNLx08ERbAaPwEUPxEUwWz8BFD8RFAEs/ETQPETQRHMxk8AxU8ERTAbPwEUPxEUwWz8BFD8RFAEs/ETQOETQiFMhk8AxU8EhTAbPgEUPxEUw2T0BFD8RBAEUPxEEAFE/EQQAUT8RBABRPxEEAFE/EQQAUT8RBABRPiEEAFE/EQQAUT8RBABRPxEEAFE/EQQASQePxFEAEnHTwQRQNLxE0EEkHT8RBABJB0/EUQAhQ8hRADFDxFEAMVPBEEAxU8EQQDFTwRBAMVPBEEAxU8EQQDFTwRBAMVPBEEAxU8EQQCFTwhBAMVPBEEAxU8EQQDFTwRBAMVPBEEAxU8EEUCHED8RRAARPxFEABE/EUQAhQ8hRADFDxFEAMUPEUQAxQ8RRADFDxFEAMUPEUQAxQ8RRADFDxFEAMUPEUQAxQ8RRADFDxFEAIUPIUQAxQ8RRADFDxFEAMUPEUQAxQ8RRADFDxFEAMUPEUQAxQ8RJB9AY0UESQbQSBFBcgE0TISQZACNEREkGUAjRARJBtD4EEGSATQ6RJBkAI0NESQZQCNDBEkG0LgQQZIBNCpEkGQAjQkRJBdAA0IISQbQaBBBkgE0FkSQZACNBBEkGUDjQARJBtAoEEGSATQGRJBkAI0AERSfZAA9fhDBnwngvLwGQJEAAgIIIIAAAggggAACCCCAAAIIIIAAAggggAACCCCAAAIIIIAAAggggAACCCCAAAIIIIAAAggggAACCCCAAAIIIIAAAggggIAAOgIggAACCCCAAAII8H92bDpw3EmUI9wAAAAASUVORK5CYII=";
export const emit = (shape, extra = {}) => ({
  op: "emitKnownFrame",
  shape,
  ...extra,
});
export const wait = (kind) => ({ op: "awaitClient", kind });

const ROOT = "$ROOT/src/example.ts";
const longLabel =
  "Inspect the bounded synthetic workspace and compare deterministic native replay presentation without executing commands";
const commandOutput = "Synthetic tool finished; no command was executed.";
const commandFor = (state, index) =>
  state.startsWith("git")
    ? "git status --short"
    : `printf '%s\\n' '${state === "tool-long-label" ? longLabel : "Inspect synthetic example " + index}'`;
export const answer = (state) => {
  if (state === "answer-table-lists")
    return "## Replay summary\n\n| State | Result |\n| --- | --- |\n| Native binding | Verified replay |\n| Storage | Durable projection |\n\n- Fixed synthetic input\n- Bounded protocol events\n\n1. Inspect\n2. Compare";
  if (state === "answer-code")
    return "Synthetic implementation:\n\n```ts\nexport const example = 2;\n```";
  if (state === "answer-mermaid")
    return "```mermaid\nflowchart LR\n  Binding --> Pump\n  Pump --> SQLite\n```";
  if (state === "jump-to-latest")
    return Array.from(
      { length: 32 },
      (_, index) =>
        "### Synthetic section " +
        (index + 1) +
        "\n\nA bounded replay paragraph verifies native scrolling and transcript layout.",
    ).join("\n\n");
  return "The synthetic example is available for native replay review.";
};

export function events(harness, state) {
  const frames = [];
  const acp = (update) => frames.push(emit("acpUpdate", { update }));
  const progress = (toolCallId, toolName) =>
    acp({
      sessionUpdate: "tool_call_update",
      toolCallId,
      status: "in_progress",
      _meta: {
        claudeCode: { toolName, toolResponse: { elapsedTimeSeconds: 1 } },
      },
    });
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
      const command = kind === "execute";
      acp({
        sessionUpdate: "tool_call",
        toolCallId: id,
        kind,
        title,
        status: "pending",
        ...(command
          ? {
              rawInput: { command: title },
              content: [],
              _meta: { claudeCode: { toolName: "Bash" } },
            }
          : {}),
        ...extra,
      });
      if (command) progress(id, "Bash");
      if (finished)
        acp({
          sessionUpdate: "tool_call_update",
          toolCallId: id,
          kind,
          status: "completed",
          ...(command
            ? {
                rawOutput: commandOutput,
                _meta: { claudeCode: { toolName: "Bash" } },
              }
            : {}),
          content: [
            {
              type: "content",
              content: {
                type: "text",
                text: command
                  ? `\`\`\`console\n${commandOutput}\n\`\`\``
                  : commandOutput,
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
        harness === "claude"
          ? commandFor(state, index)
          : state === "tool-long-label"
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
        status: "pending",
        _meta: { claudeCode: { toolName: "Edit" } },
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
      progress("fixture-edit", "Edit");
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
        status: "inProgress",
        changes: [
          {
            path: ROOT,
            kind: { type: "update" },
            diff: "@@ -1 +1 @@\n-export const example = 1;\n+export const example = 2;\n",
          },
        ],
      };
      codex("item/started", { item });
      if (state !== "file-edit-live")
        codex("item/completed", { item: { ...item, status: "completed" } });
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
    const command = commandFor(state, 1);
    const toolCall = {
      toolCallId: "fixture-tool-1",
      name: "Bash",
      kind: "execute",
      title: "Bash",
      status: "pending",
      rawInput: { command },
      content: [],
    };
    acp({
      sessionUpdate: "tool_call",
      toolCallId: toolCall.toolCallId,
      kind: "execute",
      title: command,
      status: "pending",
      rawInput: toolCall.rawInput,
      content: [],
      _meta: { claudeCode: { toolName: "Bash" } },
    });
    frames.push(
      emit("acpPermission", {
        toolCall,
        _meta: { permission: { version: 1, title: toolCall.title } },
      }),
    );
    frames.push(wait("acpPermissionDecision"));
    if (state !== "approval-declined") progress(toolCall.toolCallId, "Bash");
    const declined = state === "approval-declined";
    const output = declined
      ? "Synthetic approval declined; no command was executed."
      : commandOutput;
    acp({
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
