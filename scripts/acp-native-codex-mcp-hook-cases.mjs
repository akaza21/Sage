// Codex app-server 0.146.0 MCP progress and hook replay journeys. The MCP
// call holds between reports so its status line is visible in the app
// before the call completes; the hook journey runs one passing hook before
// a tool call and one failing hook after it.

const opening = [
  { op: "awaitClient", kind: "codexInitialize" },
  { op: "emitKnownFrame", shape: "codexInitializeResult0146" },
  { op: "awaitClient", kind: "codexStart" },
  { op: "emitKnownFrame", shape: "codexThreadResult" },
  { op: "awaitClient", kind: "codexGoalGetWithOptionalModelList" },
  { op: "emitKnownFrame", shape: "codexGoalMethodNotFound" },
  { op: "awaitClient", kind: "codexTurnStart" },
  { op: "emitKnownFrame", shape: "codexTurnResult" },
  { op: "emitKnownFrame", shape: "codexStarted" },
];

const closing = [
  { op: "emitKnownFrame", shape: "codexCompleted" },
  { op: "awaitClient", kind: "codexUnsubscribe" },
  { op: "emitKnownFrame", shape: "codexAck" },
];

/** Pause between visible states; the fixture caps one pause at 1 s. */
const HOLD_STEP_MS = 1000;
const HOLD_STEPS = 2;
const hold = Array.from({ length: HOLD_STEPS }, () => ({
  op: "pauseMs",
  ms: HOLD_STEP_MS,
}));

const native = (method, params) => ({
  op: "emitKnownFrame",
  shape: "codexNative",
  method,
  params,
});

function mcpCall(id, tool, query) {
  const item = {
    type: "mcpToolCall",
    id,
    server: "fixture-docs",
    tool,
    status: "inProgress",
    arguments: { query },
    result: null,
    error: null,
  };
  return {
    started: native("item/started", { item }),
    completed: native("item/completed", {
      item: {
        ...item,
        status: "completed",
        result: { content: [{ type: "text", text: "Synthetic MCP result" }] },
      },
    }),
  };
}

export const MCP_PROGRESS_LINES = Object.freeze([
  "Indexing synthetic docs (1 of 2)",
  "Indexing synthetic docs (2 of 2)",
]);

/** A synthetic, project-relative hook config; never a local path. */
export const HOOK_SOURCE_PATH = ".codex/hooks.json";

function hookRun(eventName, displayOrder, status, statusMessage, entries) {
  const running = status === "running";
  return {
    id: `${eventName}:${displayOrder}:${HOOK_SOURCE_PATH}`,
    eventName,
    handlerType: "command",
    executionMode: "sync",
    scope: "turn",
    sourcePath: HOOK_SOURCE_PATH,
    source: "project",
    displayOrder,
    status,
    statusMessage,
    startedAt: 1_700_000_000,
    completedAt: running ? null : 1_700_000_001,
    durationMs: running ? null : 840,
    entries,
  };
}

function hook(eventName, displayOrder, statusMessage, outcome, entries) {
  return [
    native("hook/started", {
      run: hookRun(eventName, displayOrder, "running", statusMessage, []),
    }),
    ...hold,
    native("hook/completed", {
      run: hookRun(eventName, displayOrder, outcome, statusMessage, entries),
    }),
  ];
}

export function codexMcpHookJourneys() {
  const progressCall = mcpCall(
    "fixture-mcp-progress",
    "index_fixture_docs",
    "synthetic progress",
  );
  const hookedCall = mcpCall(
    "fixture-mcp-hooked",
    "search_fixture_docs",
    "synthetic hooks",
  );
  return [
    {
      // One running MCP call that reports twice before it completes.
      id: "codex-mcp-progress",
      steps: [
        ...opening,
        progressCall.started,
        ...hold,
        ...MCP_PROGRESS_LINES.flatMap((message) => [
          native("item/mcpToolCall/progress", {
            itemId: "fixture-mcp-progress",
            message,
          }),
          ...hold,
        ]),
        progressCall.completed,
        ...closing,
      ],
    },
    {
      // A passing PreToolUse hook, the tool call, then a failing PostToolUse hook.
      id: "codex-hook-run",
      steps: [
        ...opening,
        ...hook("preToolUse", 0, "Checking synthetic policy", "completed", []),
        hookedCall.started,
        ...hold,
        hookedCall.completed,
        ...hook("postToolUse", 1, "Linting synthetic output", "failed", [
          { kind: "error", text: "Synthetic lint hook exited with status 1" },
        ]),
        ...closing,
      ],
    },
  ];
}
