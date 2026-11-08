// Replay journeys for a user-defined ACP agent. Add an agent in Settings ›
// Harnesses › Your ACP agents whose executable is the fixture binary and
// whose arguments are `--case <id>`, `--seed`, `0` (one per line); the
// fixture then speaks plain ACP as that agent would.

const wait = (kind) => ({ op: "awaitClient", kind });
const emit = (shape, extra = {}) => ({ op: "emitKnownFrame", shape, ...extra });
const say = (text) =>
  emit("acpUpdate", {
    update: {
      sessionUpdate: "agent_message_chunk",
      content: { type: "text", text },
    },
  });

const opening = [
  wait("acpInitialize"),
  emit("acpInitialize"),
  wait("acpNew"),
  emit("acpSession"),
];
const closing = [wait("acpClose"), emit("acpAck")];

function asking(params) {
  return [
    wait("acpPrompt"),
    say("Synthetic replay: the agent needs an answer before it continues."),
    emit("acpElicitation", { params }),
    wait("acpElicitationReply"),
    emit("acpElicitationEcho"),
    emit("acpCompleted"),
  ];
}

const form = {
  mode: "form",
  message: "Which synthetic branch should the replay use?",
  requestedSchema: {
    type: "object",
    properties: {
      branch: {
        type: "string",
        title: "Branch",
        oneOf: [
          { const: "main", title: "main" },
          { const: "next", title: "next" },
        ],
      },
      note: { type: "string", title: "Note for the agent" },
    },
    required: ["branch"],
  },
};

const link = {
  mode: "url",
  elicitationId: "fixture-link-1",
  message: "Open the synthetic sign-in page to continue.",
  url: "https://example.com/synthetic-sign-in",
};

export function customAcpJourneys() {
  return [
    {
      id: "custom-acp-plain",
      steps: [
        ...opening,
        wait("acpPrompt"),
        say("Synthetic replay: a plain turn from your own ACP agent."),
        emit("acpCompleted"),
        ...closing,
      ],
    },
    {
      // Answer, decline or let the card wait; the echo names the fields only.
      id: "custom-acp-elicitation-form",
      steps: [...opening, ...asking(form), ...closing],
    },
    {
      id: "custom-acp-elicitation-url",
      steps: [...opening, ...asking(link), ...closing],
    },
    {
      // The agent refuses the session until Sage authenticates with the one
      // method that needs no input, then opens it.
      id: "custom-acp-auth-required",
      steps: [
        wait("acpInitialize"),
        emit("acpInitialize", {
          authMethods: [
            { id: "fixture-browser", name: "Synthetic browser sign-in" },
          ],
        }),
        wait("acpNew"),
        emit("acpAuthRequired"),
        wait("acpAuthenticate"),
        emit("acpAck"),
        wait("acpNew"),
        emit("acpSession"),
        wait("acpPrompt"),
        say("Synthetic replay: signed in, the session opened."),
        emit("acpCompleted"),
        ...closing,
      ],
    },
    {
      // Only methods that need the user: Sage shows a notice naming them.
      id: "custom-acp-auth-notice",
      steps: [
        wait("acpInitialize"),
        emit("acpInitialize", {
          authMethods: [
            {
              id: "fixture-terminal",
              name: "Synthetic terminal login",
              type: "terminal",
              args: ["login"],
            },
            {
              id: "fixture-key",
              name: "Synthetic API key",
              type: "env_var",
              varName: "SYNTHETIC_AGENT_KEY",
            },
          ],
        }),
        wait("acpNew"),
        emit("acpAuthRequired"),
        wait("acpClose"),
      ],
    },
  ];
}
