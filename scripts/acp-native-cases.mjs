import { emit, wait, events } from "./acp-native-case-events.mjs";

const sharedStates = [
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
  "tool-long-label",
  "file-edit-live",
  "files-pill",
  "file-hunk",
  "file-diff-tab",
  "git-running",
  "git-finished",
  "plan-running",
  "plan-completed",
  "plan-mode-entry",
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
  "context-meter",
  "attachment-chip",
  "responsive-wide",
  "responsive-720",
  "responsive-480",
];
const codexStates = [
  "subagents-started",
  "subagents-finished",
  "subagents-grouped",
  "subagents-popover",
  "child-opened",
  "compaction-started",
  "compaction-completed",
  "compaction-failed",
  "compaction-cancelled",
];
const claudeStates = [
  "subagents-started",
  "subagents-finished",
  "subagents-grouped",
  "subagents-popover",
  "approval-pending",
  "approval-approved",
  "approval-declined",
];
const liveStates = new Set([
  "turn-running",
  "thinking-first-token",
  "thinking-live",
  "prose-then-thinking",
  "tool-running",
  "file-edit-live",
  "git-running",
  "subagents-started",
  "plan-running",
  "composer-stop",
  "compaction-started",
]);
export function nativeCases() {
  return ["codex", "claude"].flatMap((harness) =>
    [
      ...sharedStates,
      ...(harness === "codex" ? codexStates : claudeStates),
    ].map((state) => {
      const steps =
        harness === "codex"
          ? [
              wait("codexInitialize"),
              emit("codexInitializeResult0146"),
              wait("codexStart"),
              emit("codexThreadResult"),
              wait("codexGoalGet"),
              emit("codexGoalMethodNotFound"),
              wait("codexTurnStart"),
              emit("codexTurnResult"),
              emit("codexStarted"),
            ]
          : [
              wait("acpInitialize"),
              emit("acpInitialize"),
              wait("acpNew"),
              emit("acpSession"),
              wait("acpPrompt"),
            ];
      if (state.startsWith("approval-"))
        steps.push(emit("acpCompleted"), wait("acpPrompt"));
      steps.push(...events(harness, state));
      if (liveStates.has(state) || state === "turn-stopped")
        steps.push(wait(harness === "codex" ? "codexInterrupt" : "acpCancel"));
      if (harness === "claude")
        steps.push(
          emit(
            state === "turn-error" || state === "turn-retry"
              ? "acpError"
              : state === "turn-stopped" || liveStates.has(state)
                ? "acpCancelled"
                : "acpCompleted",
          ),
          wait("acpClose"),
          emit("acpAck"),
        );
      else
        steps.push(
          ...(liveStates.has(state) || state === "turn-stopped"
            ? []
            : [
                emit(
                  state === "turn-error" || state === "turn-retry"
                    ? "codexError"
                    : "codexCompleted",
                ),
              ]),
          wait("codexUnsubscribe"),
          emit("codexAck"),
        );
      if (state === "turn-retry") {
        steps.splice(
          -2,
          0,
          ...(harness === "claude"
            ? [
                wait("acpPrompt"),
                emit("acpUpdate", {
                  update: {
                    sessionUpdate: "agent_message_chunk",
                    content: {
                      type: "text",
                      text: "Synthetic retry completed.",
                    },
                  },
                }),
                emit("acpCompleted"),
              ]
            : [
                wait("codexTurnStart"),
                emit("codexTurnResult"),
                emit("codexStarted"),
                emit("codexDelta"),
                emit("codexCompleted"),
              ]),
        );
      }
      return { id: harness + "-" + state, steps };
    }),
  );
}
