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

export function claudeSdkJourneys() {
  return Object.entries(journeys).map(([state, steps]) => ({
    id: `claude-sdk-${state}`,
    steps: [...opening, ...steps, ...closing],
  }));
}
