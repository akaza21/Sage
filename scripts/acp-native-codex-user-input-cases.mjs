// Codex `item/tool/requestUserInput` replay journeys. The fixture asks, waits
// for the client's answer, and completes the turn by echoing the option
// labels it received (typed answers, including secrets, are never echoed).

const opening = [
  { op: "awaitClient", kind: "codexInitialize" },
  { op: "emitKnownFrame", shape: "codexInitializeResult0146" },
  { op: "awaitClient", kind: "codexStart" },
  { op: "emitKnownFrame", shape: "codexThreadResult" },
  { op: "awaitClient", kind: "codexGoalGetWithOptionalModelList" },
  { op: "emitKnownFrame", shape: "codexGoalMethodNotFound" },
];

const closing = [
  { op: "awaitClient", kind: "codexUnsubscribe" },
  { op: "emitKnownFrame", shape: "codexAck" },
];

const questions = [
  {
    id: "scope",
    header: "Scope",
    question: "Which files should the synthetic review cover?",
    isOther: false,
    isSecret: false,
    options: [
      {
        label: "Changed files",
        description: "Only files changed in this turn",
      },
      {
        label: "All files",
        description: "Every file in the synthetic project",
      },
    ],
  },
  {
    id: "token",
    header: "Token",
    question: "Synthetic access token?",
    isOther: false,
    isSecret: true,
    options: null,
  },
];

function askingTurn(autoResolutionMs) {
  return [
    { op: "awaitClient", kind: "codexTurnStart" },
    { op: "emitKnownFrame", shape: "codexTurnResult" },
    { op: "emitKnownFrame", shape: "codexStarted" },
    {
      op: "emitKnownFrame",
      shape: "codexUserInputRequest",
      questions,
      ...(autoResolutionMs ? { autoResolutionMs } : {}),
    },
    { op: "awaitClient", kind: "codexUserInputAnswer" },
    { op: "emitKnownFrame", shape: "codexUserInputEcho" },
    { op: "emitKnownFrame", shape: "codexCompleted" },
  ];
}

export function codexUserInputJourneys() {
  return [
    {
      // Asks on each of two turns and waits for the user every time.
      id: "codex-user-input",
      steps: [...opening, ...askingTurn(), ...askingTurn(), ...closing],
    },
    {
      // Asks once with a short window, so an unanswered card declines itself.
      id: "codex-user-input-auto",
      steps: [...opening, ...askingTurn(8000), ...closing],
    },
  ];
}
