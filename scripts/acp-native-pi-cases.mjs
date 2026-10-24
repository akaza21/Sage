import {
  answer,
  emit,
  wait,
  NATIVE_IMAGE_BASE64,
} from "./acp-native-case-events.mjs";
import {
  PARITY_BASE,
  PARITY_LIVE,
  SECOND_PROMPT_ANSWER,
  STREAM_BLOCKS,
} from "./acp-native-parity-events.mjs";
import { paritySupported, parityTurns } from "./capture-native-chat-parity.mjs";

export const piAbsentStates = Object.freeze([
  "tool-long-label",
  "file-edit-live",
  "files-pill",
  "file-hunk",
  "file-diff-tab",
  "git-running",
  "git-finished",
  "subagents-started",
  "subagents-finished",
  "subagents-grouped",
  "subagents-popover",
  "child-opened",
  "plan-running",
  "plan-completed",
  "plan-mode-entry",
  "approval-pending",
  "approval-approved",
  "approval-declined",
  "context-meter",
  "attachment-chip",
]);
const states = [
  "new-chat",
  "turn-running",
  "turn-completed",
  "worked-expanded",
  "jump-to-latest",
  "thinking-first-token",
  "thinking-live",
  "prose-then-thinking",
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
  "harness-menu",
  "model-menu",
  "effort-menu",
  "add-menu",
  "access-menu",
  "responsive-wide",
  "responsive-720",
  "responsive-480",
  "non-read-denial",
  "cancelled-dialog",
  ...paritySupported("pi"),
];
const live = new Set([
  ...PARITY_LIVE,
  "turn-running",
  "thinking-first-token",
  "thinking-live",
  "prose-then-thinking",
  "tool-running",
  "compaction-started",
  "composer-stop",
]);
const frame = (type, fields = {}) =>
  emit("piNative", { frame: { type, ...fields } });
const message = (content = [], stopReason = "stop") => ({
  role: "assistant",
  content,
  api: "openai-responses",
  provider: "openai",
  model: "fixture-model",
  usage: {
    input: 12,
    output: 7,
    cacheRead: 0,
    cacheWrite: 0,
    totalTokens: 19,
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
  },
  stopReason,
  timestamp: 1,
});
function assistantStream() {
  let content = [];
  const snapshot = (stopReason = "stop") =>
    message(
      content.map((block) => ({ ...block })),
      stopReason,
    );
  const update = (type, delta) => {
    const kind = type.startsWith("thinking") ? "thinking" : "text";
    if (type.endsWith("start") || content.at(-1)?.type !== kind)
      content.push({ type: kind, [kind]: "" });
    const contentIndex = content.length - 1;
    if (type.endsWith("delta")) content[contentIndex][kind] += delta;
    const partial = snapshot();
    return frame("message_update", {
      message: partial,
      assistantMessageEvent: { type, contentIndex, delta, partial },
    });
  };
  return {
    update,
    start: () => {
      content = [];
      return [
        frame("agent_start"),
        frame("turn_start"),
        frame("message_start", { message: snapshot() }),
      ];
    },
    finish: (stopReason = "stop", text = "Synthetic replay completed.") => {
      const current = content
        .filter((block) => block.type === "text")
        .map((block) => block.text)
        .join("");
      const steps =
        current === text
          ? []
          : [update("text_delta", (current ? "\n\n" : "") + text)];
      const terminal = snapshot(stopReason);
      return [
        ...steps,
        frame("message_end", { message: terminal }),
        frame("turn_end", { message: terminal, toolResults: [] }),
        frame("agent_end", { messages: [terminal] }),
        frame("agent_settled"),
      ];
    },
  };
}
function events(state, { update, finish }) {
  const steps = [];
  if (state === "answer-image")
    steps.push(
      frame("tool_execution_start", {
        toolCallId: "fixture-read-image",
        toolName: "read",
        args: { path: "$ROOT/src/example.png" },
      }),
      frame("tool_execution_end", {
        toolCallId: "fixture-read-image",
        toolName: "read",
        result: {
          content: [
            { type: "text", text: "Read image file [image/png]" },
            { type: "image", mimeType: "image/png", data: NATIVE_IMAGE_BASE64 },
          ],
        },
        isError: false,
      }),
    );
  if (state === "prose-then-thinking")
    steps.push(update("text_delta", "Inspecting the synthetic example. "));
  if (state === "motion-stream-fade")
    steps.push(...STREAM_BLOCKS.map((block) => update("text_delta", block)));
  else if (state.includes("thinking")) {
    steps.push(
      update("thinking_start", ""),
      update("thinking_delta", "Inspecting"),
    );
    if (state !== "thinking-first-token")
      steps.push(update("thinking_delta", " the bounded synthetic example."));
  } else
    steps.push(update("text_delta", "Synthetic replay.\n\n" + answer(state)));
  if (
    [
      "tool-running",
      "tool-finished",
      "tools-grouped",
      "tool-expanded",
      "worked-expanded",
    ].includes(state)
  ) {
    const count =
      state === "tools-grouped" || state === "worked-expanded" ? 3 : 1;
    for (let index = 1; index <= count; index++) {
      const toolCallId = "fixture-tool-" + index;
      steps.push(
        frame("tool_execution_start", {
          toolCallId,
          toolName: "read",
          args: {
            path:
              count === 1
                ? "$ROOT/src/example.ts"
                : `$ROOT/src/example-${index}.ts`,
          },
        }),
      );
      if (state !== "tool-running")
        steps.push(
          frame("tool_execution_end", {
            toolCallId,
            toolName: "read",
            result: {
              content: [{ type: "text", text: "export const example = 1;" }],
            },
            isError: false,
          }),
        );
    }
  }
  if (state.startsWith("compaction-")) {
    steps.push(...finish().slice(0, -1));
    steps.push(
      frame("compaction_start", { reason: "context", inputTokens: 12 }),
    );
    if (state !== "compaction-started")
      steps.push(
        frame("compaction_end", {
          result:
            state === "compaction-completed"
              ? {
                  summary: "Synthetic compacted context",
                  firstKeptEntryId: "fixture-entry-1",
                  tokensBefore: 12,
                }
              : undefined,
          aborted: state === "compaction-cancelled",
          errorMessage:
            state === "compaction-failed"
              ? "Synthetic compaction failure"
              : undefined,
        }),
      );
  }
  if (state === "non-read-denial")
    steps.push(
      frame("tool_execution_start", {
        toolCallId: "fixture-tool-1",
        toolName: "bash",
        args: { command: "synthetic command; never executed" },
      }),
      wait("piAbort"),
    );
  if (state === "cancelled-dialog")
    steps.push(
      frame("extension_ui_request", {
        id: "fixture-dialog-1",
        method: "input",
        title: "Synthetic input",
        placeholder: "Never submitted",
      }),
      wait("piDialogCancelled"),
    );
  return steps;
}
/** The terminal answer the stream settles on; streamed fades keep theirs. */
const finalText = (state) =>
  state === "motion-stream-fade"
    ? STREAM_BLOCKS.join("")
    : "Synthetic replay.\n\n" + answer(state);
export function piNativeCases() {
  return states.map((id) => {
    // Parity states drive UI over an existing state's replay.
    const state = PARITY_BASE[id] ?? id;
    const stream = assistantStream();
    const { start, update, finish } = stream;
    const steps = [
      wait("piInitialize"),
      wait("piModels"),
      wait("piNew"),
      wait("piPrompt"),
      wait("piThinkingLevels"),
      ...start(),
      ...events(state, stream),
    ];
    const holdsTurn = live.has(state) || live.has(id);
    if (holdsTurn || state === "turn-stopped") steps.push(wait("piAbort"));
    if (state === "compaction-started")
      steps.push(
        frame("compaction_end", { aborted: true }),
        frame("agent_settled"),
      );
    else if (state.startsWith("compaction-")) {
      if (state === "compaction-completed")
        steps.push(
          ...start(),
          update("text_delta", "Synthetic continuation after compaction."),
          ...finish(),
        );
      else steps.push(frame("agent_settled"));
    } else if (state === "turn-retry")
      steps.push(...finish("error").slice(0, -1));
    else
      steps.push(
        ...finish(
          state === "turn-error"
            ? "error"
            : holdsTurn ||
                state === "turn-stopped" ||
                state === "non-read-denial"
              ? "aborted"
              : "stop",
          finalText(state),
        ),
      );
    if (state === "turn-retry")
      steps.push(
        frame("auto_retry_start", {
          attempt: 1,
          maxAttempts: 3,
          delayMs: 0,
          errorMessage: "Synthetic retry",
        }),
        ...start(),
        update("text_delta", "Synthetic retry completed."),
        frame("auto_retry_end", { success: true, attempt: 1 }),
        ...finish("stop", "Synthetic retry completed."),
      );
    if (parityTurns(id) === 2)
      steps.push(
        wait("piPrompt"),
        wait("piThinkingLevels"),
        ...start(),
        update("text_delta", SECOND_PROMPT_ANSWER),
        ...finish("stop", SECOND_PROMPT_ANSWER),
      );
    steps.push(wait("piIdle"));
    return { id: "pi-" + id, steps };
  });
}

export function piIdleConfigJourney() {
  const { start, update, finish } = assistantStream();
  return {
    id: "pi-idle-config-journey",
    steps: [
      wait("piInitialize"),
      wait("piModels"),
      wait("piNew"),
      wait("piPrompt"),
      wait("piThinkingLevels"),
      ...start(),
      update("text_delta", "Synthetic first turn."),
      ...finish(),
      wait("piPrompt"),
      wait("piThinkingLevels"),
      ...start(),
      update("text_delta", "Synthetic second turn."),
      ...finish(),
      wait("piIdle"),
    ],
  };
}
