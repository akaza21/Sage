import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { nativeCases, truthfulAbsentStates } from "./acp-native-cases.mjs";
import { parseArgs } from "./capture-native-chat-contract.mjs";
import {
  actionsFor,
  namedState,
  isExpandable,
  expandExpression,
} from "./capture-native-chat-dom.mjs";
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
  const subagentSummary = readFileSync(
    new URL("../src/workbench/AgentTaskSubagentSummary.tsx", import.meta.url),
    "utf8",
  );
  assert.ok(header.includes("<AgentTaskSubagentSummary"));
  assert.ok(subagentSummary.includes("aria-label={`Recent subagents working:"));
  const source = (file) =>
    readFileSync(new URL("../src/" + file, import.meta.url), "utf8");
  const composer = source("features/chat/ChatHarnessControls.tsx");
  assert.ok(composer.includes('aria-label="Composer options"'));
  assert.ok(composer.includes('ariaLabel="Composer options menu"'));
  assert.ok(composer.includes("<ComposerAddActions"));
  const addActions = source("features/chat/ComposerAddActions.tsx");
  assert.ok(
    addActions.includes('<div role="group" aria-label="Session mode">'),
  );
  assert.ok(addActions.includes('role="menuitemradio"'));
  const access = source("features/chat/ComposerAccessControl.tsx");
  assert.ok(access.includes('aria-label="Access options"'));
  assert.ok(access.includes('ariaLabel="Access mode"'));
  const edits = source("features/chat/acp/TurnEditSummary.tsx");
  assert.ok(edits.includes('aria-label="Files edited in this chat"'));
  assert.ok(
    edits.includes("aria-label={`Preview saved diff ${qualifiedLabel}`}"),
  );
  assert.ok(
    edits.includes(
      'aria-label={`Open ${change.diffRef ? "diff" : "file"} ${qualifiedLabel}`}',
    ),
  );
  assert.ok(
    source("features/chat/acp/InlineSavedDiff.tsx").includes(
      'label="Inline saved diff"',
    ),
  );
  assert.ok(
    source("workbench/AgentArtifactPane.tsx").includes(
      'aria-label="Agent artifacts"',
    ),
  );
  assert.ok(
    source("features/diff/DiffSurface.tsx").includes(
      'label="Unified file diff"',
    ),
  );
  const tabs = source("workbench/AgentArtifactTabStrip.tsx");
  assert.ok(tabs.includes('aria-label="Agent artifact tabs"'));
  assert.ok(
    tabs.includes("const accessibleName = `${kindLabel} ${fullPath}`;"),
  );
});

test("CP-29d/e composer menus: access is its own menu, plan is a named Session mode item", async () => {
  const menu = (items) =>
    '<button aria-label="Composer options" aria-controls="options">+</button><div id="options" class="menu-popover" role="menu" aria-label="Composer options menu"><div role="group" aria-label="Session mode">' +
    items +
    "</div></div>";
  const plan = await observeMarkup(
    menu(
      '<button role="menuitemradio">Default</button><button role="menuitemradio" aria-label="Plan mode">Plan mode</button>',
    ),
  );
  assert.equal(plan.visible.addMenu, true);
  assert.equal(plan.visible.planEntry, true);
  assert.equal(plan.controls.addEnabled, true);
  const options = { harness: "claude", state: "plan-mode-entry" };
  assert.equal(namedState(options, plan).namedState, true);
  const noPlan = await observeMarkup(
    menu('<button role="menuitemradio">Default</button>'),
  );
  assert.equal(noPlan.visible.planEntry, false);
  assert.equal(namedState(options, noPlan).namedState, false);
  assert.deepEqual(actionsFor("plan-mode-entry", "claude"), [
    'button[aria-label="Composer options"]',
  ]);
  const disabled = await observeMarkup(
    '<button aria-label="Composer options" disabled>+</button><button aria-label="Access options" disabled>Ask</button>',
  );
  assert.equal(disabled.controls.addEnabled, false);
  assert.equal(disabled.controls.accessEnabled, false);
  const access = await observeMarkup(
    '<button aria-label="Access options" aria-controls="access">Ask</button>',
    '<div id="access" class="menu-popover" role="menu" aria-label="Access mode"><button role="menuitemradio">Ask each time</button></div>',
  );
  assert.equal(access.visible.addMenu, false);
  assert.equal(access.visible.accessMenu, true);
  assert.equal(
    namedState({ harness: "pi", state: "access-menu" }, access).namedState,
    true,
  );
});

test("CP-32c header resources popover is owned by the focused chat", async () => {
  const popover = await observeMarkup(
    "",
    '<header><button aria-label="Show chat resources" aria-controls="resources"></button></header><div id="resources" class="menu-popover" role="dialog" aria-label="Chat resources"><section aria-label="Subagents"><span aria-label="Recent subagents working: 0">0 working</span><div class="agent-task-header__agent-list"><p class="agent-task-header__agent">Subagent done</p></div></section></div>',
  );
  assert.equal(popover.visible.popover, true);
  assert.equal(popover.visible.subagentCounts, true);
  assert.equal(popover.visible.subagentEntries, true);
});

test("file-hunk and file-diff-tab use the chat-wide summary and the named Diff tab", async () => {
  const zone =
    '<section class="acp-turn" data-turn-id="turn"></section><div class="chat-edit-summary-zone"><section aria-label="Files edited in this chat"><button aria-expanded="true">1 file changed</button><ul><li><button aria-label="Open diff project / src/example.ts"></button><button aria-label="Preview saved diff project / src/example.ts" aria-expanded="true">Preview saved diff</button><div role="table" aria-label="Inline saved diff">@@</div></li></ul></section></div>';
  const hunk = await observeMarkup(zone);
  assert.equal(hunk.visible.hunk, true);
  assert.equal(
    namedState({ harness: "claude", state: "file-hunk" }, hunk).namedState,
    true,
  );
  const tab = await observeMarkup(
    zone,
    '<aside class="agent-artifact-pane" data-owner-chat-id="capture-chat"><nav aria-label="Agent artifact tabs"><div role="presentation" data-resource-kind="diff"><button role="tab" aria-label="Diff src/example.ts"></button></div></nav></aside>',
  );
  assert.equal(tab.visible.diffTab, true);
  const diffTab = { harness: "claude", state: "file-diff-tab" };
  // The lazy Diff surface still shows "Loading Diff…": not yet capturable.
  assert.equal(tab.visible.diffView, false);
  assert.equal(namedState(diffTab, tab).namedState, false);
  const loaded = await observeMarkup(
    zone,
    '<aside class="agent-artifact-pane" aria-label="Agent artifacts" data-owner-chat-id="capture-chat"><nav aria-label="Agent artifact tabs"><div role="presentation" data-resource-kind="diff"><button role="tab" aria-label="Diff src/example.ts"></button></div></nav><section aria-label="Diff for src/example.ts"><div role="table" aria-label="Unified file diff">@@</div></section></aside>',
  );
  assert.equal(loaded.visible.diffView, true);
  assert.equal(namedState(diffTab, loaded).namedState, true);
  const [header, preview] = actionsFor("file-hunk", "claude");
  assert.equal(isExpandable(header), true);
  assert.equal(isExpandable(preview), true);
  assert.equal(isExpandable(actionsFor("file-diff-tab", "claude")[1]), false);
  let clicks = 0;
  const button = {
    disabled: false,
    expanded: "true",
    hasAttribute: () => true,
    getAttribute() {
      return this.expanded;
    },
    scrollIntoView() {},
    click() {
      clicks += 1;
      this.expanded = this.expanded === "true" ? "false" : "true";
    },
  };
  const run = () =>
    new Function("document", "return " + expandExpression(header))({
      querySelectorAll: (selector) =>
        selector.includes("[aria-controls]") ? [] : [button],
    });
  assert.equal(run(), true);
  assert.equal(clicks, 0);
  button.expanded = "false";
  assert.equal(run(), true);
  assert.equal(clicks, 1);
});

test("pre-session composer stays out of the idle add-menu path", () => {
  // CP-29e: Composer options is disabled until a session offers actions, so
  // add-menu submits a Turn and waits for the enabled trigger first.
  assert.deepEqual(actionsFor("add-menu", "claude"), [
    'button[aria-label="Composer options"]',
  ]);
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

async function observeMarkup(markup, outsideMarkup = "", state = "completed") {
  const window = new JSDOM(
    `<section class="agent-chat-pane agent-chat-pane--focused">${markup}</section>${outsideMarkup}`,
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
                state,
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

test("observations distinguish rendered assistant text from visible chat lazy fallbacks", async () => {
  const transcript =
    '<div class="acp-chat-projection"><section class="acp-turn" data-turn-id="turn"><div class="acp-message--assistant"><div class="acp-markdown">Synthetic completed answer.</div></div></section>';
  for (const [status, expected] of [
    ['<p role="status">Loading Chat history\u2026</p>', true],
    ['<p role="status">Loading Markdown...</p>', true],
    [
      '<p role="status" style="visibility:hidden">Loading Chat history\u2026</p>',
      false,
    ],
    [
      '<details><summary>Details</summary><p role="status">Loading Chat history\u2026</p></details>',
      false,
    ],
    ['<p role="status">Completed</p>', false],
    ["", false],
  ]) {
    const observation = await observeMarkup(transcript + status + "</div>");
    assert.equal(observation.assistantRendered, true);
    assert.equal(observation.lazyFallbackVisible, expected, status);
    assert.equal(
      namedState({ harness: "claude", state: "turn-completed" }, observation)
        .namedState,
      !expected,
    );
  }
  const loading = await observeMarkup(
    '<div class="chat-surface"><p role="status">Loading Chat history\u2026</p></div>',
  );
  assert.equal(loading.assistantRendered, false);
  assert.equal(loading.lazyFallbackVisible, true);
  const unrelated = await observeMarkup(
    transcript + '</div><p role="status">Loading unrelated panel\u2026</p>',
    '<section class="agent-chat-pane"><div class="chat-surface"><p role="status">Loading Chat history\u2026</p></div></section>',
  );
  assert.equal(unrelated.assistantRendered, true);
  assert.equal(unrelated.lazyFallbackVisible, false);
});

test("terminal witnesses belong to the observed Turn, never the last rendered older Turn", async () => {
  for (const [state, turnState, kind, markup] of [
    [
      "turn-error",
      "failed",
      "failure",
      '<div class="acp-turn__failure">Turn failed</div>',
    ],
    [
      "turn-stopped",
      "cancelled",
      "stopped",
      '<span class="acp-turn__reason" role="status">Stopped</span>',
    ],
    [
      "plan-completed",
      "completed",
      "plan",
      '<div class="acp-plan">Plan completed</div>',
    ],
    [
      "compaction-completed",
      "completed",
      "compaction",
      '<div class="acp-compaction-notice">Compacted</div>',
    ],
    [
      "subagents-finished",
      "completed",
      "subagent",
      '<div class="acp-subagent-row">Subagent finished</div>',
    ],
  ]) {
    const old =
      '<section class="acp-turn" data-turn-id="previous">' +
      markup +
      "</section>";
    const options = { harness: "claude", state };
    for (const current of [
      "",
      '<section class="acp-turn" data-turn-id="turn"></section>',
    ]) {
      const stale = await observeMarkup(old + current, "", turnState);
      stale.compactionStates = ["completed"];
      assert.equal(stale.assistantRendered, false);
      assert.equal(stale.lazyFallbackVisible, false);
      assert.equal(stale.visible[kind], false, state);
      assert.equal(namedState(options, stale).namedState, false, state);
    }
    const rendered = await observeMarkup(
      old +
        '<section class="acp-turn" data-turn-id="turn">' +
        markup +
        "</section>",
      "",
      turnState,
    );
    rendered.compactionStates = ["completed"];
    assert.equal(rendered.assistantRendered, false);
    assert.equal(rendered.visible[kind], true, state);
    assert.equal(namedState(options, rendered).namedState, true, state);
  }
});

test("access-menu opens the anchored access menu for every harness", () => {
  for (const harness of ["codex", "claude", "pi"]) {
    assert.deepEqual(actionsFor("access-menu", harness), [
      'button[aria-label="Access options"]',
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
      lazyFallbackVisible: false,
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
      assistantRendered: true,
      lazyFallbackVisible: false,
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
