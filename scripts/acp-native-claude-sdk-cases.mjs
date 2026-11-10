// Claude Code 2.1.319 Agent SDK (stream-json) replay journeys. Frames keep
// the fields a live CLI trace carries for each event, with synthetic text,
// ids and project-relative paths, so the SDK binding renders them through
// the same bridge a real session uses.

const frame = (value) => ({
  op: "emitKnownFrame",
  shape: "claudeSdkNative",
  frame: { session_id: "fixture-session", parent_tool_use_id: null, ...value },
});
const wait = (kind) => ({ op: "awaitClient", kind });

const EXAMPLE = "$ROOT/src/example.ts";
const messageStart = (id) =>
  frame({
    type: "stream_event",
    event: {
      type: "message_start",
      message: { id, type: "message", role: "assistant", content: [] },
    },
  });
const delta = (type, text) =>
  frame({
    type: "stream_event",
    event: {
      type: "content_block_delta",
      index: 0,
      delta:
        type === "thinking"
          ? { type: "thinking_delta", thinking: text }
          : { type: "text_delta", text },
    },
  });
const assistant = (id, content, extra = {}) =>
  frame({
    type: "assistant",
    message: { id, type: "message", role: "assistant", content },
    ...extra,
  });
const toolUse = (id, name, input) =>
  assistant(`msg_fixture_${id}`, [
    { type: "tool_use", id: `toolu_fixture_${id}`, name, input },
  ]);
const toolResult = (id, content, isError = false) =>
  frame({
    type: "user",
    message: {
      role: "user",
      content: [
        {
          type: "tool_result",
          tool_use_id: `toolu_fixture_${id}`,
          content,
          is_error: isError,
        },
      ],
    },
  });
const result = (extra = {}) =>
  frame({
    type: "result",
    subtype: "success",
    is_error: false,
    stop_reason: "end_turn",
    result: "",
    usage: {
      input_tokens: 12,
      output_tokens: 48,
      cache_read_input_tokens: 1024,
      cache_creation_input_tokens: 256,
    },
    modelUsage: { "claude-fixture": { contextWindow: 200000 } },
    ...extra,
  });
const reply = (id, text) => [
  messageStart(id),
  delta("text", text),
  assistant(id, [{ type: "text", text }]),
];

const ANSWER = "The synthetic example is available for native replay review.";
const opening = [
  wait("claudeSdkInitialize"),
  wait("claudeSdkSettings"),
  wait("claudeSdkPrompt"),
];
const closing = [wait("claudeSdkIdle")];

// A message sent while Claude works, reported as Claude Code 2.1.322 reports
// its queued input: `queued` on arrival, `started` when a run takes it up,
// `completed` once it ran. Stop withdraws it first (`cancelled`).
const followUp = [wait("claudeSdkFollowUp"), lifecycle("queued")];
function lifecycle(state) {
  return { op: "emitKnownFrame", shape: "claudeSdkLifecycle", state };
}
// The reply quotes what the prompt carried, block by block.
const ECHO = "$PROMPT_ECHO";
const echo = (id) => [
  messageStart(id),
  delta("text", ECHO),
  assistant(id, [{ type: "text", text: ECHO }]),
];
const LONG_REPLY = [
  "Reviewing the synthetic example section by section.",
  " The first section declares the example constant.",
  " The second section exports it for the replay.",
  " The third section is still being read.",
];
const longReply = (id) => [
  messageStart(id),
  ...LONG_REPLY.map((text) => delta("text", text)),
];
const compact = {
  op: "awaitClient",
  kind: "claudeSdkPrompt",
  promptText: "/compact",
};

const journeys = {
  "turn-completed": [
    messageStart("msg_fixture_answer"),
    delta("thinking", "Checking the synthetic example before answering."),
    delta("text", ANSWER),
    assistant("msg_fixture_answer", [{ type: "text", text: ANSWER }]),
    result(),
  ],
  "file-edit": [
    toolUse("read", "Read", { file_path: EXAMPLE }),
    toolResult("read", "1\texport const example = 1;\n"),
    toolUse("edit", "Edit", {
      file_path: EXAMPLE,
      old_string: "export const example = 1;",
      new_string: "export const example = 2;",
      replace_all: false,
    }),
    toolResult("edit", `The file ${EXAMPLE} has been updated successfully.`),
    ...reply("msg_fixture_done", "Updated the synthetic example to 2."),
    result(),
  ],
  "tool-approval": [
    toolUse("bash", "Bash", {
      command: "git status --short",
      description: "Show working tree status",
    }),
    frame({
      type: "control_request",
      request_id: "fixture-permission-1",
      request: {
        subtype: "can_use_tool",
        tool_name: "Bash",
        input: {
          command: "git status --short",
          description: "Show working tree status",
        },
        permission_suggestions: [
          {
            type: "addRules",
            rules: [{ toolName: "Bash", ruleContent: "git status:*" }],
            behavior: "allow",
            destination: "localSettings",
          },
        ],
        tool_use_id: "toolu_fixture_bash",
      },
    }),
    wait("claudeSdkPermission"),
    toolResult("bash", "Synthetic tool finished; no command was executed."),
    ...reply("msg_fixture_done", "The working tree is clean."),
    result(),
  ],
  plan: [
    toolUse("todo", "TodoWrite", {
      todos: [
        {
          content: "Inspect the synthetic example",
          activeForm: "Inspecting the synthetic example",
          status: "completed",
        },
        {
          content: "Update the synthetic example",
          activeForm: "Updating the synthetic example",
          status: "in_progress",
        },
        {
          content: "Verify the replay",
          activeForm: "Verifying the replay",
          status: "pending",
        },
      ],
    }),
    toolResult("todo", "Todos have been modified successfully."),
    ...reply("msg_fixture_done", "The plan is in place."),
    result(),
  ],
  "turn-stopped": [
    messageStart("msg_fixture_live"),
    delta("text", "Reviewing the synthetic example"),
    wait("claudeSdkInterrupt"),
    result({
      subtype: "error_during_execution",
      is_error: true,
      stop_reason: null,
    }),
  ],
  "turn-error": [
    messageStart("msg_fixture_partial"),
    delta("text", "Starting the synthetic review."),
    result({
      subtype: "error_during_execution",
      is_error: true,
      stop_reason: null,
      result: "API Error: 529 Overloaded. The synthetic service is busy.",
    }),
  ],
  // Send a message while the read runs: Claude takes it up at the tool
  // boundary and answers it in the same turn.
  "follow-up-tool": [
    toolUse("read", "Read", { file_path: EXAMPLE }),
    ...followUp,
    toolResult("read", "1\texport const example = 1;\n"),
    lifecycle("started"),
    ...echo("msg_fixture_follow_up"),
    result(),
    lifecycle("completed"),
  ],
  // Send a message while the reply streams: the reply's run ends, then the
  // message runs as the turn's next run.
  "follow-up-reply": [
    ...longReply("msg_fixture_long"),
    ...followUp,
    assistant("msg_fixture_long", [
      { type: "text", text: LONG_REPLY.join("") },
    ]),
    result(),
    lifecycle("started"),
    ...echo("msg_fixture_follow_up"),
    result(),
    lifecycle("completed"),
  ],
  // Send a message while the reply streams, then Stop: Sage withdraws the
  // message before it interrupts, so it never runs.
  "follow-up-stop": [
    ...longReply("msg_fixture_long"),
    ...followUp,
    wait("claudeSdkInterrupt"),
    result({
      subtype: "error_during_execution",
      is_error: true,
      stop_reason: null,
    }),
  ],
  // Compact after one answered turn, in the 2.1.322 frames the SDK
  // binding's runtime tests record: modelled, since capturing a real one
  // takes a model call. The summary stays Claude Code's own.
  compact: [
    ...reply("msg_fixture_answer", ANSWER),
    result(),
    compact,
    frame({ type: "system", subtype: "status", status: "compacting" }),
    frame({
      type: "system",
      subtype: "status",
      status: null,
      compact_result: "success",
    }),
    frame({
      type: "system",
      subtype: "compact_boundary",
      compact_metadata: { trigger: "manual", pre_tokens: 1296 },
    }),
    frame({
      type: "user",
      isReplay: false,
      message: {
        role: "user",
        content:
          "This session is being continued from a previous conversation. Summary: the synthetic example was reviewed.",
      },
    }),
    frame({
      type: "user",
      isReplay: true,
      message: {
        role: "user",
        content: "<local-command-stdout>Compacted </local-command-stdout>",
      },
    }),
    result({ num_turns: 0 }),
  ],
  // Each prompt is answered with what it carried, so an @ mention of a
  // credential file shows Sage's "Not attached" note and a file too large to
  // attach shows its @path. Two prompts, then the chat closes.
  "prompt-echo": [
    ...echo("msg_fixture_echo_1"),
    result(),
    wait("claudeSdkPrompt"),
    ...echo("msg_fixture_echo_2"),
    result(),
  ],
  "sign-in": [
    assistant(
      "msg_fixture_auth",
      [{ type: "text", text: "Not logged in · Please run /login" }],
      { error: "authentication_failed" },
    ),
    result({
      is_error: true,
      result: "Not logged in · Please run /login",
    }),
  ],
};

// Compact in a new chat, as Claude Code 2.1.322 answers it: a local error
// reply, and a turn that ends without an error.
const compactEmpty = [
  wait("claudeSdkInitialize"),
  wait("claudeSdkSettings"),
  compact,
  frame({
    type: "assistant",
    message: {
      id: "msg_fixture_compact",
      type: "message",
      role: "assistant",
      model: "<synthetic>",
      stop_reason: "end_turn",
      stop_sequence: null,
      content: [{ type: "text", text: "Error: No messages to compact" }],
    },
    local_command_source:
      "<local-command-stderr>Error: No messages to compact</local-command-stderr>",
    local_command_run: { command: "compact", args: "" },
    local_command_outcome: { kind: "failed" },
  }),
  result({
    num_turns: 0,
    stop_reason: null,
    local_command: "compact",
    usage: {
      input_tokens: 0,
      output_tokens: 0,
      cache_read_input_tokens: 0,
      cache_creation_input_tokens: 0,
    },
    modelUsage: {},
  }),
  ...closing,
];

export function claudeSdkJourneys() {
  return [
    ...Object.entries(journeys).map(([state, steps]) => ({
      id: `claude-sdk-${state}`,
      steps: [...opening, ...steps, ...closing],
    })),
    { id: "claude-sdk-compact-empty", steps: compactEmpty },
  ];
}
