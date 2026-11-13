// Codex app-server notice replay journey. A `configWarning` lands
// right after initialize, before any thread is bound, so Sage holds it and
// delivers it when thread/start responds; a `model/rerouted` then arrives
// during the turn.

const native = (shape, method, params) => ({
  op: "emitKnownFrame",
  shape,
  method,
  params,
});

/** A synthetic, project-relative config file; never a local path. */
export const CONFIG_WARNING_PATH = ".codex/config.toml";

export function codexNoticeJourneys() {
  return [
    {
      id: "codex-startup-notices",
      steps: [
        { op: "awaitClient", kind: "codexInitialize" },
        { op: "emitKnownFrame", shape: "codexInitializeResultObserved" },
        native("codexNativeThreadless", "configWarning", {
          summary: "Invalid synthetic configuration; using defaults.",
          details: "unknown variant `turbo` for key `model_reasoning_effort`",
          path: CONFIG_WARNING_PATH,
          range: {
            start: { line: 3, column: 1 },
            end: { line: 3, column: 32 },
          },
        }),
        { op: "awaitClient", kind: "codexStart" },
        { op: "emitKnownFrame", shape: "codexThreadResult" },
        { op: "awaitClient", kind: "codexGoalGetWithOptionalModelList" },
        { op: "emitKnownFrame", shape: "codexGoalMethodNotFound" },
        { op: "awaitClient", kind: "codexTurnStart" },
        { op: "emitKnownFrame", shape: "codexTurnResult" },
        { op: "emitKnownFrame", shape: "codexStarted" },
        native("codexNative", "model/rerouted", {
          fromModel: "fixture-model",
          toModel: "fixture-model-safe",
          reason: "highRiskCyberActivity",
        }),
        { op: "emitKnownFrame", shape: "codexCompleted" },
        { op: "awaitClient", kind: "codexUnsubscribe" },
        { op: "emitKnownFrame", shape: "codexAck" },
      ],
    },
  ];
}
