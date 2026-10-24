// Replay events for the waves 6-8 parity states (capture-native-chat-parity.mjs).
// Most states drive UI over an existing replay, so they reuse that state's
// wire events; tool, subagent and fork states add only shapes the pinned
// Codex 0.146 schema or claude-agent-acp 0.76.0 emit. Nothing is executed.
import { emit, wait } from "./acp-native-case-events.mjs";

/** Parity states that replay an existing state's first-Turn events. */
export const PARITY_BASE = Object.freeze({
  "selection-popover": "turn-completed",
  "annotation-field": "turn-completed",
  "annotation-chip": "turn-completed",
  "annotation-sent": "turn-completed",
  "side-chat-open": "turn-completed",
  "side-chat-close-confirm": "turn-completed",
  "turn-navigator-hover": "turn-completed",
  "find-in-chat": "turn-completed",
  "fork-menu": "turn-completed",
  "fork-dialog": "turn-completed",
  "fork-thread": "turn-completed",
  "approval-card": "approval-pending",
  "subagents-name-hover": "subagents-grouped",
  "subagent-child-header": "child-opened",
  "motion-thinking-shimmer": "thinking-live",
  "motion-worked-expand": "worked-expanded",
  "motion-jump-pill": "jump-to-latest",
});

/** Parity states whose Turn stays live until Stop or teardown. */
export const PARITY_LIVE = Object.freeze([
  "motion-thinking-shimmer",
  "motion-stream-fade",
  "subagents-tab-active-done",
]);

export const SECOND_PROMPT_ANSWER =
  "Synthetic second replay answer. It cites Annotation 1 when one was sent.";
export const STREAM_BLOCKS = Object.freeze([
  "Synthetic streamed replay opens with a first bounded paragraph.",
  "\n\nA second paragraph lands in a later chunk.",
  "\n\n- A streamed list row\n- Another streamed list row",
  "\n\nThe final streamed paragraph stays live until Stop.",
]);
const FORK_SESSION = "fixture-fork-session-1";

const codexFrame = (method, params) => emit("codexNative", { method, params });
const acpFrame = (update) => emit("acpUpdate", { update });

function codexMcp(id, tool, outcome) {
  const item = {
    type: "mcpToolCall",
    id,
    status: "inProgress",
    server: "fixture-docs",
    tool,
    arguments: { query: "synthetic native replay" },
  };
  return [
    codexFrame("item/started", { item }),
    codexFrame("item/completed", {
      item:
        outcome === "error"
          ? {
              ...item,
              status: "failed",
              error: { message: "Synthetic MCP probe error" },
            }
          : {
              ...item,
              status: "completed",
              result: {
                content: [{ type: "text", text: "Synthetic MCP result" }],
                structuredContent: {
                  hits: [{ title: "Synthetic doc", path: "docs/example.md" }],
                },
              },
            },
    }),
  ];
}

function codexWebSearch(id) {
  const item = { type: "webSearch", id, query: "" };
  return [
    codexFrame("item/started", { item }),
    codexFrame("item/completed", {
      item: {
        ...item,
        query: "synthetic native replay",
        action: { type: "search", query: "synthetic native replay" },
      },
    }),
  ];
}

function codexCommand(id, command) {
  const item = {
    type: "commandExecution",
    id,
    command,
    status: "inProgress",
    cwd: "$ROOT",
    commandActions: [],
  };
  return [
    codexFrame("item/started", { item }),
    codexFrame("item/completed", {
      item: {
        ...item,
        status: "completed",
        aggregatedOutput: "Synthetic tool finished; no command was executed.",
        exitCode: 0,
      },
    }),
  ];
}

function claudeWebSearch(id) {
  return [
    acpFrame({
      sessionUpdate: "tool_call",
      toolCallId: id,
      kind: "fetch",
      title: '"synthetic native replay"',
      status: "pending",
      rawInput: { query: "synthetic native replay" },
    }),
    acpFrame({
      sessionUpdate: "tool_call_update",
      toolCallId: id,
      kind: "fetch",
      status: "completed",
      content: [
        {
          type: "content",
          content: { type: "text", text: "Synthetic search result summary." },
        },
      ],
    }),
  ];
}

function claudeCommand(id) {
  const command = "printf '%s\\n' 'Inspect synthetic example 1'";
  const output = "Synthetic tool finished; no command was executed.";
  const meta = { claudeCode: { toolName: "Bash" } };
  return [
    acpFrame({
      sessionUpdate: "tool_call",
      toolCallId: id,
      kind: "execute",
      title: command,
      status: "pending",
      rawInput: { command },
      content: [],
      _meta: meta,
    }),
    acpFrame({
      sessionUpdate: "tool_call_update",
      toolCallId: id,
      kind: "execute",
      status: "completed",
      rawOutput: output,
      _meta: meta,
      content: [
        {
          type: "content",
          content: { type: "text", text: "```console\n" + output + "\n```" },
        },
      ],
    }),
  ];
}

function codexChild(index, finished) {
  const child = "fixture-child-" + index;
  return [
    codexFrame(finished ? "item/completed" : "item/started", {
      threadId: "$SESSION",
      item: {
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
      },
    }),
    codexFrame("item/completed", {
      threadId: "$SESSION",
      item: {
        id: "fixture-child-activity-" + index,
        type: "subAgentActivity",
        agentThreadId: child,
        kind: finished ? "completed" : "started",
      },
    }),
  ];
}

function streamed(harness) {
  return STREAM_BLOCKS.map((delta) =>
    harness === "claude"
      ? acpFrame({
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: delta },
        })
      : codexFrame("item/agentMessage/delta", {
          itemId: "fixture-answer",
          delta,
        }),
  );
}

/** First-Turn events for parity states with their own wire shapes. */
export function parityOwnEvents(harness, state) {
  if (state === "motion-stream-fade") return streamed(harness);
  if (harness === "claude") {
    if (state === "tool-web-search") return claudeWebSearch("fixture-web-1");
    if (state === "tool-mixed-group")
      return [
        ...claudeCommand("fixture-tool-1"),
        ...claudeWebSearch("fixture-web-1"),
      ];
  }
  if (harness === "codex") {
    if (state === "tool-mcp-result")
      return codexMcp("fixture-mcp-1", "search_fixture_docs", "result");
    if (state === "tool-mcp-error")
      return codexMcp("fixture-mcp-1", "probe_fixture_docs", "error");
    if (state === "tool-web-search") return codexWebSearch("fixture-web-1");
    if (state === "tool-mixed-group")
      return [
        ...codexCommand("fixture-tool-1", "Inspect synthetic example 1"),
        ...codexWebSearch("fixture-web-1"),
        ...codexMcp("fixture-mcp-1", "search_fixture_docs", "result"),
      ];
    if (state === "subagents-tab-active-done")
      return [...codexChild(1, true), ...codexChild(2, false)];
  }
  throw new Error("parity-events-unsupported: " + harness + "-" + state);
}

/** Claude fork states negotiate unstable `sessionCapabilities.fork`. */
export function parityInitializeShape(state) {
  return state.startsWith("fork-") ? "acpInitializeFork" : "acpInitialize";
}

/** A second, completed Turn for states that send again. */
export function paritySecondTurn(harness) {
  if (harness === "claude")
    return [
      wait("acpPrompt"),
      acpFrame({
        sessionUpdate: "agent_message_chunk",
        content: { type: "text", text: SECOND_PROMPT_ANSWER },
      }),
      emit("acpCompleted"),
    ];
  return [
    wait("codexTurnStart"),
    emit("codexTurnResult"),
    emit("codexStarted"),
    codexFrame("item/agentMessage/delta", {
      itemId: "fixture-answer-2",
      delta: SECOND_PROMPT_ANSWER,
    }),
    emit("codexCompleted"),
  ];
}

/**
 * The native fork RPC after the Turn settles. Codex then releases the loaded
 * fork thread with `thread/unsubscribe` once the fork Chat commits; Claude's
 * 0.76.0 fork is a stored copy with nothing to release.
 */
export function parityForkSteps(harness) {
  return harness === "claude"
    ? [wait("acpFork"), emit("acpForkResult")]
    : [
        wait("codexFork"),
        emit("codexForkResult"),
        wait("codexUnsubscribe"),
        emit("codexAck"),
      ];
}

export const PARITY_FORK_SESSION = FORK_SESSION;
