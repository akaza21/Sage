// Codex steer replay journey. The turn holds open until the user sends a
// second message while the agent works; Codex accepts the steer, echoes it as
// its own userMessage item and answers it. The transcript must show the
// steered message once, as the user's bubble between the two answers.

const native = (method, item) => ({
  op: "emitKnownFrame",
  shape: "codexNative",
  method,
  params: { item },
});

/** The message the real-app check types into the composer mid-turn. */
export const CODEX_STEER_TEXT = "Synthetic steer: use the other example.";

export function codexSteerJourneys() {
  const echo = {
    type: "userMessage",
    id: "fixture-steer-echo",
    content: [{ type: "text", text: CODEX_STEER_TEXT, text_elements: [] }],
  };
  return [
    {
      id: "codex-steered-message",
      steps: [
        { op: "awaitClient", kind: "codexInitialize" },
        { op: "emitKnownFrame", shape: "codexInitializeResultObserved" },
        { op: "awaitClient", kind: "codexStart" },
        { op: "emitKnownFrame", shape: "codexThreadResult" },
        { op: "awaitClient", kind: "codexGoalGetWithOptionalModelList" },
        { op: "emitKnownFrame", shape: "codexGoalMethodNotFound" },
        { op: "awaitClient", kind: "codexTurnStart" },
        { op: "emitKnownFrame", shape: "codexTurnResult" },
        { op: "emitKnownFrame", shape: "codexStarted" },
        native("item/completed", {
          type: "agentMessage",
          id: "fixture-steer-before",
          text: "Working on the example; send a steer.",
        }),
        { op: "awaitClient", kind: "codexSteer" },
        { op: "emitKnownFrame", shape: "codexSteerResult" },
        native("item/started", echo),
        native("item/completed", echo),
        native("item/completed", {
          type: "agentMessage",
          id: "fixture-steer-after",
          text: "Switched to the other example after your steer.",
        }),
        { op: "emitKnownFrame", shape: "codexCompleted" },
        { op: "awaitClient", kind: "codexUnsubscribe" },
        { op: "emitKnownFrame", shape: "codexAck" },
      ],
    },
  ];
}
