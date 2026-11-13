import {
  answer,
  emit,
  wait,
  NATIVE_IMAGE_BASE64,
} from "./acp-native-case-events.mjs";
import {
  PARITY_BASE,
  PARITY_LIVE,
  SECOND_PROMPT_ANSWER,
  STREAM_BLOCKS,
  STREAM_CHUNKS,
  paced,
} from "./acp-native-parity-events.mjs";
import { paritySupported, parityTurns } from "./capture-native-chat-parity.mjs";

export const piAbsentStates = Object.freeze([
  "tool-long-label",
  "file-edit-live",
  "files-pill",
  "file-hunk",
  "file-diff-tab",
  "git-running",
  "git-finished",
  "subagents-started",
  "subagents-finished",
  "subagents-grouped",
  "subagents-popover",
  "child-opened",
  "plan-running",
  "plan-completed",
  "plan-mode-entry",
  "approval-pending",
  "approval-approved",
  "approval-declined",
  "context-meter",
  "attachment-chip",
]);
const states = [
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
  "compaction-started",
  "compaction-completed",
  "compaction-failed",
  "compaction-cancelled",
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
  "responsive-wide",
  "responsive-720",
  "responsive-480",
  "non-read-denial",
  "cancelled-dialog",
  ...paritySupported("pi"),
];
const live = new Set([
  ...PARITY_LIVE,
  "turn-running",
  "thinking-first-token",
  "thinking-live",
  "prose-then-thinking",
  "tool-running",
  "compaction-started",
  "composer-stop",
]);
const frame = (type, fields = {}) =>
  emit("piNative", { frame: { type, ...fields } });
const SMALL_USAGE = { input: 12, output: 7, cacheRead: 0, cacheWrite: 0 };
const message = (content = [], stopReason = "stop", usage = SMALL_USAGE) => ({
  role: "assistant",
  content,
  api: "openai-responses",
  provider: "openai",
  model: "fixture-model",
  usage: {
    ...usage,
    totalTokens:
      usage.input + usage.output + usage.cacheRead + usage.cacheWrite,
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
  },
  stopReason,
  timestamp: 1,
});
function assistantStream(usage) {
  let content = [];
  const snapshot = (stopReason = "stop") =>
    message(
      content.map((block) => ({ ...block })),
      stopReason,
      usage,
    );
  const update = (type, delta) => {
    const kind = type.startsWith("thinking") ? "thinking" : "text";
    if (type.endsWith("start") || content.at(-1)?.type !== kind)
      content.push({ type: kind, [kind]: "" });
    const contentIndex = content.length - 1;
    if (type.endsWith("delta")) content[contentIndex][kind] += delta;
    return frame("message_update", {
      usage: snapshot().usage,
      assistantMessageEvent: { type, contentIndex, delta },
    });
  };
  return {
    update,
    start: () => {
      content = [];
      return [
        frame("agent_start"),
        frame("turn_start"),
        frame("message_start", { message: snapshot() }),
      ];
    },
    finish: (stopReason = "stop", text = "Synthetic replay completed.") => {
      const current = content
        .filter((block) => block.type === "text")
        .map((block) => block.text)
        .join("");
      const steps =
        current === text
          ? []
          : [update("text_delta", (current ? "\n\n" : "") + text)];
      const terminal = snapshot(stopReason);
      return [
        ...steps,
        frame("message_end", { message: terminal }),
        frame("turn_end", { message: terminal, toolResults: [] }),
        frame("agent_end", { messages: [terminal] }),
        frame("agent_settled"),
      ];
    },
  };
}
function events(state, { update, finish }) {
  const steps = [];
  if (state === "answer-image")
    steps.push(
      frame("tool_execution_start", {
        toolCallId: "fixture-read-image",
        toolName: "read",
        args: { path: "$ROOT/src/example.png" },
      }),
      frame("tool_execution_end", {
        toolCallId: "fixture-read-image",
        toolName: "read",
        result: {
          content: [
            { type: "text", text: "Read image file [image/png]" },
            { type: "image", mimeType: "image/png", data: NATIVE_IMAGE_BASE64 },
          ],
        },
        isError: false,
      }),
    );
  if (state === "prose-then-thinking")
    steps.push(update("text_delta", "Inspecting the synthetic example. "));
  if (state === "motion-stream-fade")
    steps.push(
      ...paced(STREAM_CHUNKS.map((chunk) => update("text_delta", chunk))),
    );
  else if (state.includes("thinking")) {
    steps.push(
      update("thinking_start", ""),
      update("thinking_delta", "Inspecting"),
    );
    if (state !== "thinking-first-token")
      steps.push(update("thinking_delta", " the bounded synthetic example."));
  } else
    steps.push(update("text_delta", "Synthetic replay.\n\n" + answer(state)));
  if (
    [
      "tool-running",
      "tool-finished",
      "tools-grouped",
      "tool-expanded",
      "worked-expanded",
    ].includes(state)
  ) {
    const count =
      state === "tools-grouped" || state === "worked-expanded" ? 3 : 1;
    for (let index = 1; index <= count; index++) {
      const toolCallId = "fixture-tool-" + index;
      steps.push(
        frame("tool_execution_start", {
          toolCallId,
          toolName: "read",
          args: {
            path:
              count === 1
                ? "$ROOT/src/example.ts"
                : `$ROOT/src/example-${index}.ts`,
          },
        }),
      );
      if (state !== "tool-running")
        steps.push(
          frame("tool_execution_end", {
            toolCallId,
            toolName: "read",
            result: {
              content: [{ type: "text", text: "export const example = 1;" }],
            },
            isError: false,
          }),
        );
    }
  }
  if (state.startsWith("compaction-")) {
    steps.push(...finish().slice(0, -1));
    steps.push(
      frame("compaction_start", { reason: "context", inputTokens: 12 }),
    );
    if (state !== "compaction-started")
      steps.push(
        frame("compaction_end", {
          result:
            state === "compaction-completed"
              ? {
                  summary: "Synthetic compacted context",
                  firstKeptEntryId: "fixture-entry-1",
                  tokensBefore: 12,
                }
              : undefined,
          aborted: state === "compaction-cancelled",
          errorMessage:
            state === "compaction-failed"
              ? "Synthetic compaction failure"
              : undefined,
        }),
      );
  }
  if (state === "non-read-denial")
    steps.push(
      frame("tool_execution_start", {
        toolCallId: "fixture-tool-1",
        toolName: "bash",
        args: { command: "synthetic command; never executed" },
      }),
      wait("piAbort"),
    );
  // A select with nothing to choose cannot be shown, so Sage cancels it.
  if (state === "cancelled-dialog")
    steps.push(
      frame("extension_ui_request", {
        id: "fixture-dialog-1",
        method: "select",
        title: "Synthetic choice",
        options: [],
      }),
      wait("piDialogCancelled"),
    );
  return steps;
}
/** The terminal answer the stream settles on; streamed fades keep theirs. */
const finalText = (state) =>
  state === "motion-stream-fade"
    ? STREAM_BLOCKS.join("")
    : "Synthetic replay.\n\n" + answer(state);
export function piNativeCases() {
  return states.map((id) => {
    // Parity states drive UI over an existing state's replay.
    const state = PARITY_BASE[id] ?? id;
    const stream = assistantStream();
    const { start, update, finish } = stream;
    const steps = [
      wait("piInitialize"),
      wait("piModels"),
      wait("piNew"),
      wait("piPrompt"),
      wait("piThinkingLevels"),
      ...start(),
      ...events(state, stream),
    ];
    const holdsTurn = live.has(state) || live.has(id);
    if (holdsTurn || state === "turn-stopped") steps.push(wait("piAbort"));
    if (state === "compaction-started")
      steps.push(
        frame("compaction_end", { aborted: true }),
        frame("agent_settled"),
      );
    else if (state.startsWith("compaction-")) {
      if (state === "compaction-completed")
        steps.push(
          ...start(),
          update("text_delta", "Synthetic continuation after compaction."),
          ...finish(),
        );
      else steps.push(frame("agent_settled"));
    } else if (state === "turn-retry")
      steps.push(...finish("error").slice(0, -1));
    else
      steps.push(
        ...finish(
          state === "turn-error"
            ? "error"
            : holdsTurn ||
                state === "turn-stopped" ||
                state === "non-read-denial"
              ? "aborted"
              : "stop",
          finalText(state),
        ),
      );
    if (state === "turn-retry")
      steps.push(
        frame("auto_retry_start", {
          attempt: 1,
          maxAttempts: 3,
          delayMs: 0,
          errorMessage: "Synthetic retry",
        }),
        ...start(),
        update("text_delta", "Synthetic retry completed."),
        frame("auto_retry_end", { success: true, attempt: 1 }),
        ...finish("stop", "Synthetic retry completed."),
      );
    // Pi discovers thinking levels once per activation, never per prompt,
    // so the second Turn awaits only its prompt.
    if (parityTurns(id) === 2)
      steps.push(
        wait("piPrompt"),
        ...start(),
        update("text_delta", SECOND_PROMPT_ANSWER),
        ...finish("stop", SECOND_PROMPT_ANSWER),
      );
    steps.push(wait("piIdle"));
    return { id: "pi-" + id, steps };
  });
}

export function piIdleConfigJourney() {
  const { start, update, finish } = assistantStream();
  return {
    id: "pi-idle-config-journey",
    steps: [
      wait("piInitialize"),
      wait("piModels"),
      wait("piNew"),
      wait("piPrompt"),
      wait("piThinkingLevels"),
      ...start(),
      update("text_delta", "Synthetic first turn."),
      ...finish(),
      wait("piPrompt"),
      wait("piThinkingLevels"),
      ...start(),
      update("text_delta", "Synthetic second turn."),
      ...finish(),
      wait("piIdle"),
    ],
  };
}

/**
 * Tool calls replayed through the bundled Sage bridge extension. Each
 * piBridgeTool step plays one Pi tool call: the child emits the extension's
 * sage.pi.prepare.v1 / sage.pi.commit.v1 dialogs and blocks on Sage's replies,
 * so Approve and Deny run through the real approval card and write path. The
 * Turn must run with workspace-write access; read-only Turns refuse every
 * prepare. Paths target the seeded disposable project (src/example.ts). A
 * replay step fails the case if Sage approves it, and a step with
 * expectRefusal fails it unless Sage answers with exactly that refusal.
 */
export function piBridgeJourneys() {
  const call = (index, tool, args, extra = {}) => ({
    op: "awaitClient",
    kind: "piBridgeTool",
    toolCallId: `fixture-bridge-${extra.toolIndex ?? index}`,
    tool,
    args,
    prepareId: `fixture-prepare-${index}`,
    commitId: `fixture-commit-${index}`,
    ...(extra.replay ? { replay: true } : {}),
    ...(extra.expectRefusal ? { expectRefusal: extra.expectRefusal } : {}),
  });
  const journeys = {
    "write-new": [
      call(1, "write", {
        path: "src/bridge-note.txt",
        content: "Created through the Sage bridge.\n",
      }),
    ],
    "edit-existing": [
      call(1, "edit", {
        path: "src/example.ts",
        oldText: "example = 1",
        newText: "example = 2",
      }),
    ],
    "git-config-refused": [
      call(1, "write", {
        path: ".git/config",
        content: "[core]\n\tfsmonitor = synthetic\n",
      }),
    ],
    "device-name-refused": [
      call(1, "write", { path: "nul.txt", content: "Never written.\n" }),
    ],
    "replay-refused": [
      call(1, "write", {
        path: "src/bridge-replay.txt",
        content: "First approved write.\n",
      }),
      call(
        2,
        "write",
        { path: "src/bridge-replay.txt", content: "Replayed write.\n" },
        {
          toolIndex: 1,
          replay: true,
          expectRefusal: "Sage refused: this tool call was already handled",
        },
      ),
    ],
  };
  return Object.entries(journeys).map(([name, calls]) => {
    const { start, update, finish } = assistantStream();
    return {
      id: "pi-bridge-" + name,
      steps: [
        wait("piInitialize"),
        wait("piModels"),
        wait("piNew"),
        wait("piPrompt"),
        wait("piThinkingLevels"),
        ...start(),
        update("text_delta", "Synthetic bridge edit."),
        ...calls,
        ...finish("stop", "Synthetic bridge edit."),
        wait("piIdle"),
      ],
    };
  });
}

/**
 * One Pi 1.5 Turn through every surface the real app should show: thinking,
 * a read tool, a bridge write approval, an extension notice, a session
 * title, an extension question answered on the question card, a steer that Pi delivers at the next model turn, and usage large
 * enough to move the context meter. The child holds the Turn open until
 * Sage sends the steer, and the ended Turn must clear Pi's queue.
 */
export function piFullTurnJourney() {
  const first = assistantStream();
  const second = assistantStream({
    input: 9000,
    output: 600,
    cacheRead: 3000,
    cacheWrite: 400,
  });
  const steered = {
    role: "user",
    content: [{ type: "text", text: "Synthetic steer." }],
    timestamp: 2,
  };
  return {
    id: "pi-150-full-turn",
    steps: [
      wait("piInitialize"),
      wait("piModels"),
      wait("piNew"),
      wait("piPrompt"),
      wait("piThinkingLevels"),
      ...first.start(),
      first.update("thinking_start", ""),
      first.update("thinking_delta", "Checking the synthetic example first."),
      first.update("text_delta", "Reading the example, then adding a note."),
      frame("tool_execution_start", {
        toolCallId: "fixture-read-1",
        toolName: "read",
        args: { path: "$ROOT/src/example.ts" },
      }),
      frame("tool_execution_end", {
        toolCallId: "fixture-read-1",
        toolName: "read",
        result: {
          content: [{ type: "text", text: "export const example = 1;" }],
        },
        isError: false,
      }),
      {
        op: "awaitClient",
        kind: "piBridgeTool",
        toolCallId: "fixture-bridge-1",
        tool: "write",
        args: {
          path: "src/pi-note.txt",
          content: "Created through the Sage bridge on Pi 1.5.\n",
        },
        prepareId: "fixture-prepare-1",
        commitId: "fixture-commit-1",
      },
      frame("extension_ui_request", {
        id: "fixture-notify-1",
        method: "notify",
        message: "Synthetic extension notice.",
        notifyType: "info",
      }),
      frame("session_info_changed", { name: "Synthetic Pi session" }),
      frame("extension_ui_request", {
        id: "fixture-question-1",
        method: "editor",
        title: "Describe the note",
        prefill: "A short note",
      }),
      wait("piDialogAnswered"),
      first.update("text_delta", " Waiting for a steer."),
      wait("piSteer"),
      frame("queue_update", { steering: ["Synthetic steer."], followUp: [] }),
      ...first
        .finish(
          "toolUse",
          "Reading the example, then adding a note. Waiting for a steer.",
        )
        .slice(0, 2),
      frame("queue_update", { steering: [], followUp: [] }),
      frame("message_start", { message: steered }),
      frame("message_end", { message: steered }),
      ...second.start().slice(1),
      second.update("text_delta", "Adjusted after your steer."),
      ...second.finish("stop", "Adjusted after your steer."),
      wait("piClearQueue"),
      wait("piIdle"),
    ],
  };
}

/**
 * A run whose `agent_end` outgrows a frame, then a second prompt. Pi's
 * `agent_end` repeats every message of the run; here the run read the
 * synthetic example in 24 pages of 48 KiB, as Pi's read tool pages a large
 * file, so the repeat is about 1.2 MiB. Sage reads past it, settles the
 * Turn from the streamed reply, and the chat takes the next prompt.
 */
export function piLargeRunJourney() {
  const PAGES = 24;
  const PAGE_BYTES = 48 * 1024;
  const LINE = "export const example = 1;\n";
  const first = assistantStream();
  const second = assistantStream();
  const pages = Array.from({ length: PAGES }, (_, index) => ({
    toolCallId: `fixture-read-${index + 1}`,
    toolName: "read",
    args: {
      path: "$ROOT/src/example.ts",
      offset: index * 2000 + 1,
      limit: 2000,
    },
  }));
  const page = (call) => ({
    role: "toolResult",
    toolCallId: call.toolCallId,
    toolName: call.toolName,
    content: [{ type: "text", text: "" }],
    isError: false,
    timestamp: 1,
  });
  const fill = (pointers) => ({ pointers, unit: LINE, bytes: PAGE_BYTES });
  const reads = pages.flatMap((call) => [
    frame("tool_execution_start", call),
    {
      ...frame("tool_execution_end", {
        ...call,
        result: { content: [{ type: "text", text: "" }] },
        isError: false,
      }),
      fill: fill(["/result/content/0/text"]),
    },
  ]);
  const answer = `Read all ${PAGES} pages of the synthetic example.`;
  const finish = first.finish("stop", answer);
  const end = finish.findIndex((step) => step.frame.type === "agent_end");
  const terminal = finish[end].frame.messages;
  const calls = message(
    pages.map((call) => ({
      type: "toolCall",
      id: call.toolCallId,
      name: call.toolName,
      arguments: call.args,
    })),
    "toolUse",
  );
  finish[end] = {
    ...frame("agent_end", {
      messages: [calls, ...pages.map(page), ...terminal],
    }),
    fill: fill(
      pages.map((_, index) => `/messages/${index + 1}/content/0/text`),
    ),
  };
  const next = "Synthetic second turn after the large run.";
  return {
    id: "pi-large-run-end",
    steps: [
      wait("piInitialize"),
      wait("piModels"),
      wait("piNew"),
      wait("piPrompt"),
      wait("piThinkingLevels"),
      ...first.start(),
      first.update("text_delta", "Reading the synthetic example page by page."),
      ...reads,
      ...finish,
      wait("piPrompt"),
      ...second.start(),
      second.update("text_delta", next),
      ...second.finish("stop", next),
      wait("piIdle"),
    ],
  };
}

/**
 * A Pi that loaded prompt templates and skills, as `get_commands` lists
 * them, one with a name long enough to test how the / menu fits it. Each
 * prompt is answered with what Pi received, so a picked command shows the
 * `/name arguments` text Sage sent. Two prompts, then the chat closes.
 */
export function piCommandsJourney() {
  const commands = [
    {
      name: "fix-tests",
      description: "Fix the failing tests in the current project",
      source: "prompt",
      location: "project",
    },
    {
      name: "summarize-the-current-branch-changes-for-a-pull-request-description",
      description:
        "Summarize the branch's changes as a pull request description",
      source: "prompt",
      location: "user",
    },
    {
      name: "skill:lint",
      description: "Lint the files changed in this session",
      source: "skill",
      location: "project",
      path: "$ROOT/.pi/skills/lint/SKILL.md",
    },
    {
      name: "skill:release-notes",
      description: "Draft release notes from the commit log",
      source: "skill",
      location: "user",
      path: "$ROOT/.pi/skills/release-notes/SKILL.md",
    },
  ];
  const turn = () => {
    const stream = assistantStream();
    return [
      ...stream.start(),
      stream.update("text_delta", "$PROMPT_ECHO"),
      ...stream.finish("stop", "$PROMPT_ECHO"),
    ];
  };
  return {
    id: "pi-commands",
    steps: [
      { ...wait("piInitialize"), commands },
      wait("piModels"),
      wait("piNew"),
      wait("piPrompt"),
      wait("piThinkingLevels"),
      ...turn(),
      wait("piPrompt"),
      ...turn(),
      wait("piIdle"),
    ],
  };
}
