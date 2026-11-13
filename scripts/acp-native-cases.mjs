import {
  emit,
  wait,
  events,
  acpEffortUpdate,
} from "./acp-native-case-events.mjs";
import {
  paritySupported,
  parityAbsent,
  parityGaps,
  parityTurns,
} from "./capture-native-chat-parity.mjs";
import {
  PARITY_BASE,
  PARITY_LIVE,
  parityOwnEvents,
  parityOwnsBase,
  parityInitializeShape,
  paritySecondTurn,
  parityForkSteps,
} from "./acp-native-parity-events.mjs";

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
  "compaction-completed",
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
  ...PARITY_LIVE,
]);
export const truthfulAbsentStates = Object.freeze({
  pi: Object.freeze([...piAbsentStates, ...parityAbsent("pi")]),
  codex: Object.freeze([
    "compaction-started",
    "compaction-failed",
    "compaction-cancelled",
    "approval-pending",
    "approval-approved",
    "approval-declined",
    ...parityAbsent("codex"),
  ]),
  claude: Object.freeze([
    "child-opened",
    "compaction-started",
    "compaction-completed",
    "compaction-failed",
    "compaction-cancelled",
    "context-meter",
    ...parityAbsent("claude"),
  ]),
});
export const nativeCaptureProductGaps = Object.freeze({
  codex: Object.freeze([
    "model-menu",
    "attachment-chip",
    "file-edit-live",
    "file-hunk",
    "file-diff-tab",
    ...parityGaps("codex"),
  ]),
  claude: Object.freeze(["attachment-chip", ...parityGaps("claude")]),
  pi: Object.freeze([...parityGaps("pi")]),
});
/** A state's first-Turn wire events, through its parity base when it has one. */
function caseEvents(harness, state) {
  if (PARITY_BASE[state] && !parityOwnsBase(harness, state))
    return events(harness, PARITY_BASE[state]);
  if (paritySupported(harness).includes(state))
    return parityOwnEvents(harness, state);
  return events(harness, state);
}
export function nativeCases() {
  return [
    ...piNativeCases(),
    ...["codex", "claude"].flatMap((harness) => {
      const states = [
        ...sharedStates,
        ...(harness === "codex" ? codexStates : claudeStates),
        ...paritySupported(harness),
      ].filter((state) => !truthfulAbsentStates[harness].includes(state));
      return states.map((state) => {
        const steps =
          harness === "codex"
            ? [
                wait("codexInitialize"),
                emit("codexInitializeResultObserved"),
                wait("codexStart"),
                emit(
                  ["effort-menu", "model-menu"].includes(state)
                    ? "codexThreadEffortResult"
                    : "codexThreadResult",
                ),
                ...(["effort-menu", "model-menu"].includes(state)
                  ? [
                      wait("codexModelList"),
                      emit("codexModelListEffortResult"),
                      wait("codexGoalGet"),
                    ]
                  : [wait("codexGoalGetWithOptionalModelList")]),
                emit("codexGoalMethodNotFound"),
                wait("codexTurnStart"),
                emit("codexTurnResult"),
                emit("codexStarted"),
              ]
            : [
                wait("acpInitialize"),
                emit(parityInitializeShape(state)),
                wait("acpNew"),
                emit("acpSession"),
                wait("acpPrompt"),
                ...(state === "effort-menu" ? [acpEffortUpdate()] : []),
              ];
        if (state.startsWith("approval-"))
          steps.push(
            emit("acpUpdate", {
              update: {
                sessionUpdate: "agent_message_chunk",
                content: {
                  type: "text",
                  text: "Synthetic approval setup completed. No tool was run.",
                },
              },
            }),
            emit("acpCompleted"),
            wait("acpPrompt"),
          );
        steps.push(...caseEvents(harness, state));
        if (liveStates.has(state) || state === "turn-stopped")
          steps.push(
            wait(harness === "codex" ? "codexInterrupt" : "acpCancel"),
          );
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
        // Second Turns and the fork RPC land after the first Turn settles,
        // before the closing request the tail awaits.
        if (parityTurns(state) === 2)
          steps.splice(-2, 0, ...paritySecondTurn(harness));
        if (state === "fork-thread")
          steps.splice(-2, 0, ...parityForkSteps(harness));
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
      });
    }),
  ];
}
import { piNativeCases, piAbsentStates } from "./acp-native-pi-cases.mjs";
