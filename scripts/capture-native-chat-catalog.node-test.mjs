import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { nativeCases, truthfulAbsentStates } from "./acp-native-cases.mjs";
import { parseArgs } from "./capture-native-chat-contract.mjs";
import { actionsFor, namedState } from "./capture-native-chat-dom.mjs";
import { observationFor, HARNESS } from "./capture-native-chat-dom.mjs";
import { NATIVE_IMAGE_BASE64 } from "./acp-native-case-events.mjs";
import { JSDOM } from "jsdom";
import { waitForComposerIdle } from "./capture-native-chat-drive.mjs";

test("capture accessible selectors stay anchored to product controls", () => {
  const effort = readFileSync(
    new URL("../src/features/chat/ComposerEffortControl.tsx", import.meta.url),
    "utf8",
  );
  assert.ok(effort.includes('title="Select effort"'));
  assert.ok(effort.includes('aria-haspopup="dialog"'));
  const header = readFileSync(
    new URL("../src/workbench/AgentTaskHeader.tsx", import.meta.url),
    "utf8",
  );
  assert.ok(header.includes('aria-label="Show chat resources"'));
  assert.ok(header.includes('ariaLabel="Chat resources"'));
  assert.ok(header.includes("aria-label={`Recent subagents working:"));
});

test("effort readiness does not depend on the no-account Codex model picker", async () => {
  const samples = [
    {
      state: "running",
      controls: { stop: true, send: false, effortEnabled: false },
    },
    {
      state: "completed",
      controls: { stop: false, send: true, effortEnabled: false },
    },
    {
      state: "completed",
      controls: {
        stop: false,
        send: true,
        effortEnabled: true,
        modelEnabled: false,
      },
    },
  ];
  const result = await waitForComposerIdle(
    async () => ({
      turnId: "turn",
      bindingId: HARNESS.codex.binding,
      ...samples.shift(),
    }),
    "observation()",
    "turn",
    HARNESS.codex.binding,
    Date.now() + 1000,
    () => {},
    "effortEnabled",
  );
  assert.equal(samples.length, 0);
  assert.equal(result.controls.modelEnabled, false);
});

test("Codex subagents-started includes distinct admitted native ownership before cancellation", () => {
  const { steps } = nativeCases().find(
    ({ id }) => id === "codex-subagents-started",
  );
  const spawn = steps.find(
    ({ params }) => params?.item?.type === "collabAgentToolCall",
  );
  const activity = steps.find(
    ({ params }) => params?.item?.type === "subAgentActivity",
  );
  assert.ok(activity);
  assert.equal(spawn.method, "item/started");
  assert.equal(activity.method, "item/completed");
  assert.equal(activity.params.threadId, "$SESSION");
  assert.equal(activity.params.item.kind, "started");
  assert.equal(
    activity.params.item.agentThreadId,
    spawn.params.item.receiverThreadIds[0],
  );
  assert.notEqual(activity.params.item.id, spawn.params.item.id);
  assert.equal(steps.indexOf(activity), steps.indexOf(spawn) + 1);
  assert.ok(
    steps.indexOf(activity) <
      steps.findIndex(({ kind }) => kind === "codexInterrupt"),
  );
});

async function observeMarkup(markup) {
  const window = new JSDOM(
    `<section class="agent-chat-pane agent-chat-pane--focused">${markup}</section>`,
    { runScripts: "outside-only" },
  ).window;
  try {
    Object.defineProperty(window.HTMLElement.prototype, "innerText", {
      get() {
        return this.textContent;
      },
    });
    window.HTMLElement.prototype.getClientRects = () => [{}];
    window.HTMLElement.prototype.getBoundingClientRect = () => ({
      x: 0,
      y: 0,
      width: 100,
      height: 20,
    });
    window.workbench = {
      useWorkbenchStore: {
        getState: () => ({
          activeTabId: "capture-chat",
          project: { id: "project" },
          tabs: [{ id: "capture-chat", kind: "chat" }],
        }),
      },
    };
    window.registry = {
      acpProjectionStoreFor: () => ({
        getState: () => ({
          page: {
            chatId: "capture-chat",
            projectId: "project",
            turns: [
              {
                id: "turn",
                sessionId: "session",
                state: "completed",
                items: [],
              },
            ],
            sessions: [{ id: "session", bindingId: HARNESS.claude.binding }],
          },
        }),
      }),
    };
    window.owner = { pinnedAgentChatOwner: () => "capture-chat" };
    return await window.eval(
      observationFor(HARNESS.claude.binding)
        .replace("await import('/src/app/store.ts')", "workbench")
        .replace(
          "await import('/src/app/acpProjectionRegistry.ts')",
          "registry",
        )
        .replace("await import('/src/app/agentChatOwner.ts')", "owner"),
    );
  } finally {
    window.close();
  }
}

test("observation sees the current effort slider and shared Access options trigger", async () => {
  const observation = await observeMarkup(
    '<button aria-haspopup="dialog" title="Select effort">Medium</button><div class="composer__effort-menu" role="dialog"><input type="range" aria-label="Thinking level"></div><button aria-label="Access options">Read-only</button>',
  );
  assert.equal(observation.controls.effort, true);
  assert.equal(observation.controls.effortEnabled, true);
  assert.equal(observation.controls.access, true);
  assert.equal(observation.visible.effortMenu, true);
  assert.equal(observation.visible.effortSlider, true);
  const withdrawn = await observeMarkup(
    '<button aria-haspopup="dialog" title="Select effort" disabled>Medium</button><div class="composer__effort-menu" role="dialog"></div>',
  );
  assert.equal(withdrawn.controls.effortEnabled, false);
  assert.equal(withdrawn.visible.effortSlider, false);
});

test("observation distinguishes CP-32c header counts and loaded rows from the child pane", async () => {
  const child =
    '<aside class="subagent-surface"><div class="subagent-surface__transcript"><div class="acp-message">Child answer</div></div></aside>';
  const pane = await observeMarkup(child);
  assert.equal(pane.visible.popover, false);
  assert.equal(pane.visible.child, true);
  const loading =
    '<div role="dialog" aria-label="Chat resources"><section aria-label="Subagents"><div aria-label="Recent subagents working: 0">0 working 1 done</div><p role="status">Loading subagents</p></section></div>';
  const pending = await observeMarkup(loading);
  assert.equal(pending.visible.popover, true);
  assert.equal(pending.visible.subagentCounts, true);
  assert.equal(pending.visible.subagentEntries, false);
  const loaded = await observeMarkup(
    loading.replace(
      '<p role="status">Loading subagents</p>',
      '<div class="agent-task-header__agent-list"><p class="agent-task-header__agent">Subagent done</p></div>',
    ),
  );
  assert.equal(loaded.visible.subagentEntries, true);
});

test("file captures distinguish live Editing files from the composer-adjacent completed pill", async () => {
  const transcript =
    '<section class="acp-turn" data-turn-id="turn"><section class="acp-file-changes">Old transcript summary</section><div class="acp-tool-rollup" data-activity-kind="edit" data-activity-state="inProgress">Editing files</div></section>';
  const live = await observeMarkup(transcript);
  assert.equal(live.visible.files, false);
  assert.equal(live.visible.editingFiles, true);
  assert.equal(
    namedState(
      { harness: "claude", state: "file-edit-live" },
      {
        ...live,
        state: "running",
        itemKinds: ["fileChangeDiff"],
      },
    ).namedState,
    true,
  );
  assert.equal(
    namedState(
      { harness: "claude", state: "file-edit-live" },
      {
        ...live,
        state: "running",
        itemKinds: ["fileChangeDiff"],
        visible: { files: true, editingFiles: false },
      },
    ).namedState,
    false,
  );
  const pill = await observeMarkup(
    transcript +
      '<div class="chat-edit-summary-zone"><section class="acp-file-changes"><button class="acp-file-changes__header">1 file changed +1 -1</button></section></div>',
  );
  assert.equal(pill.visible.files, true);
  assert.equal(
    namedState({ harness: "claude", state: "files-pill" }, pill).namedState,
    true,
  );
  assert.equal(
    pill.elements.find(({ kind }) => kind === "files").text,
    "1 file changed +1 -1",
  );
});

test("Pi captures the shared access picker without claiming Ask or write authority", () => {
  for (const harness of ["codex", "claude", "pi"]) {
    assert.deepEqual(actionsFor("access-menu", harness), [
      'button[aria-label="Access options"]',
      'button[aria-label="Permission mode"]',
    ]);
    assert.equal(
      parseArgs([
        "--harness",
        harness,
        "--state",
        "access-menu",
        "--width",
        "480",
      ]).caseId,
      `${harness}-access-menu`,
    );
  }
  for (const state of [
    "approval-pending",
    "approval-approved",
    "approval-declined",
  ])
    assert.ok(truthfulAbsentStates.pi.includes(state));
});

test("subagents-popover opens header counts without selecting a child pane", () => {
  for (const harness of ["codex", "claude"]) {
    assert.deepEqual(actionsFor("subagents-popover", harness), [
      'button[aria-label="Show chat resources"]',
    ]);
    const options = { harness, state: "subagents-popover" };
    const observation = {
      state: "completed",
      visible: {},
      controls: {},
      itemKinds: [],
    };
    for (const visible of [
      { child: true },
      { popover: true },
      { popover: true, subagentCounts: true },
      { subagentCounts: true, subagentEntries: true },
    ])
      assert.equal(
        namedState(options, { ...observation, visible }).namedState,
        false,
      );
    assert.equal(
      namedState(options, {
        ...observation,
        visible: { popover: true, subagentCounts: true, subagentEntries: true },
      }).namedState,
      true,
    );
  }
  assert.ok(
    actionsFor("child-opened", "codex").includes(
      ".acp-turn:last-of-type .acp-subagent-row",
    ),
  );
});

test("Pi answer-image is a correlated read-tool image, never an assistant image block", () => {
  const options = parseArgs([
    "--harness",
    "pi",
    "--state",
    "answer-image",
    "--width",
    "480",
  ]);
  assert.equal(options.caseId, "pi-answer-image");
  const { steps } = nativeCases().find(({ id }) => id === options.caseId);
  const start = steps.find(
    ({ frame }) => frame?.type === "tool_execution_start",
  );
  const end = steps.find(({ frame }) => frame?.type === "tool_execution_end");
  assert.equal(start.frame.toolName, "read");
  assert.equal(start.frame.args.path, "$ROOT/src/example.png");
  assert.equal(end.frame.toolCallId, start.frame.toolCallId);
  assert.equal(end.frame.toolName, "read");
  assert.equal(end.frame.isError, false);
  assert.deepEqual(end.frame.result.content, [
    { type: "text", text: "Read image file [image/png]" },
    { type: "image", mimeType: "image/png", data: NATIVE_IMAGE_BASE64 },
  ]);
  assert.ok(steps.indexOf(start) < steps.indexOf(end));
  assert.ok(
    steps.indexOf(end) <
      steps.findIndex(({ frame }) => frame?.type === "agent_end"),
  );
  for (const { frame } of steps)
    assert.equal(
      frame?.message?.content?.some(({ type }) => type === "image") ?? false,
      false,
    );
});

test("every admitted harness captures its advertised effort slider", () => {
  for (const harness of ["codex", "claude", "pi"]) {
    assert.equal(truthfulAbsentStates[harness].includes("effort-menu"), false);
    for (const width of ["1888", "720", "480"])
      assert.equal(
        parseArgs([
          "--harness",
          harness,
          "--state",
          "effort-menu",
          "--width",
          width,
        ]).caseId,
        `${harness}-effort-menu`,
      );
    assert.deepEqual(actionsFor("effort-menu", harness), [
      'button[aria-haspopup="dialog"][title="Select effort"]',
    ]);
    const observation = {
      state: "completed",
      visible: { effortMenu: true },
      controls: {},
      itemKinds: [],
    };
    const options = { harness, state: "effort-menu" };
    assert.equal(namedState(options, observation).namedState, false);
    assert.equal(
      namedState(options, {
        ...observation,
        visible: { effortMenu: true, effortSlider: true },
      }).namedState,
      true,
    );
  }
  const { steps } = nativeCases().find(({ id }) => id === "claude-effort-menu");
  const notification = steps.find(
    ({ update }) => update?.sessionUpdate === "config_option_update",
  );
  assert.ok(notification);
  const effort = notification.update.configOptions.find(
    ({ category }) => category === "thought_level",
  );
  assert.equal(effort.type, "select");
  assert.equal(effort.currentValue, "medium");
  assert.deepEqual(
    effort.options.map(({ value }) => value),
    ["low", "medium", "high"],
  );
  assert.ok(
    steps.indexOf(notification) >
      steps.findIndex(({ kind }) => kind === "acpPrompt"),
  );
  assert.ok(
    steps.indexOf(notification) <
      steps.findIndex(({ shape }) => shape === "acpCompleted"),
  );
  const pi = nativeCases().find(({ id }) => id === "pi-effort-menu");
  assert.ok(pi.steps.some(({ kind }) => kind === "piThinkingLevels"));
});
