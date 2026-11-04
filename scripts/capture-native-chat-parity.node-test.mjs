import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import path from "node:path";
import { JSDOM } from "jsdom";
import {
  PARITY_STATES,
  PARITY_STATE_NAMES,
  parityAbsent,
  parityCell,
  parityGaps,
  parityMatrixRow,
  parityMotion,
  paritySupported,
  parityTurns,
  parityWidths,
} from "./capture-native-chat-parity.mjs";
import {
  nativeCases,
  truthfulAbsentStates,
  nativeCaptureProductGaps,
} from "./acp-native-cases.mjs";
import { parseArgs } from "./capture-native-chat-contract.mjs";
import {
  HARNESS,
  namedState,
  observationFor,
  scopeToTurn,
} from "./capture-native-chat-dom.mjs";
import {
  FORK_QUIET_MS,
  PARITY_DOCUMENT_SELECTORS,
  PARITY_PANE_SELECTORS,
  parityChatPredicates,
  parityRules,
  parityStepsFor,
  phraseRectExpression,
  SELECTED_PHRASE,
} from "./capture-native-chat-parity-dom.mjs";
import {
  centerExpression,
  driveParitySteps,
  fillExpression,
  presentExpression,
  scopedClickExpression,
} from "./capture-native-chat-parity-drive.mjs";
import {
  captureBurst,
  seekExpression,
  validateBurstSpec,
} from "./capture-native-chat-motion.mjs";
import { burstManifest } from "./capture-native-chat-evidence.mjs";
import { keyParams, mouseParams } from "./source-functional-native-ide-cdp.mjs";

const HARNESSES = ["codex", "claude", "pi"];
const source = (file) =>
  readFileSync(new URL("../src/" + file, import.meta.url), "utf8");

test("every parity cell is supported, truthfully absent or a gap, and the CLI agrees", () => {
  const ids = new Set(nativeCases().map(({ id }) => id));
  for (const { state, harnesses, live, spec } of PARITY_STATES) {
    assert.ok(
      live.length > 0 && live.every((ref) => /^s[1-8]\/f\d{4}$/u.test(ref)),
      state,
    );
    assert.match(spec, /^\d+(?:\.\d+)?$/u);
    for (const harness of HARNESSES) {
      const cell = harnesses[harness];
      const supported = paritySupported(harness).includes(state);
      const absent = parityAbsent(harness).includes(state);
      const gap = parityGaps(harness).includes(state);
      assert.equal(supported + absent + gap, 1, harness + "/" + state);
      assert.equal(
        ids.has(harness + "-" + state),
        supported,
        harness + "/" + state,
      );
      assert.equal(truthfulAbsentStates[harness].includes(state), absent);
      assert.equal(nativeCaptureProductGaps[harness].includes(state), gap);
      if (!supported) assert.ok((cell.absent ?? cell.gap).length > 20);
      for (const width of [1888, 720, 480]) {
        const args = [
          "--harness",
          harness,
          "--state",
          state,
          "--width",
          String(width),
        ];
        if (absent)
          assert.throws(() => parseArgs(args), /truthful-absent-state/u);
        else if (gap)
          assert.throws(() => parseArgs(args), /product-gap-state/u);
        else if (!parityWidths(state).includes(width))
          assert.throws(() => parseArgs(args), /state-width-unsupported/u);
        else assert.equal(parseArgs(args).caseId, harness + "-" + state);
      }
    }
  }
  // Truthful reasons the owner asked for: Pi has no fork or subagents.
  for (const state of ["fork-menu", "fork-dialog", "fork-thread"])
    assert.ok(parityAbsent("pi").includes(state));
  for (const state of [
    "subagents-name-hover",
    "subagents-tab-active-done",
    "subagent-child-header",
  ])
    assert.ok(parityAbsent("pi").includes(state));
  assert.deepEqual(parityGaps("codex"), ["side-chat-running"]);
});

test("the acceptance matrix carries exactly the catalogue's parity rows and references", () => {
  const matrix = readFileSync(
    new URL(
      "../docs/work-items/CODEX-PARITY/acceptance-matrix.md",
      import.meta.url,
    ),
    "utf8",
  );
  const rows = new Map(
    matrix
      .split(/\r?\n/u)
      .filter((line) => line.startsWith("| `"))
      .map((line) => [line.split("|")[1].trim().slice(1, -1), line]),
  );
  for (const state of PARITY_STATE_NAMES) {
    assert.equal(rows.get(state), parityMatrixRow(state), state);
    for (const harness of HARNESSES)
      assert.ok(parityCell(state, harness).includes("refs live s"), state);
  }
  assert.match(parityCell("turn-navigator-hover", "codex"), /width 1888 only/u);
  assert.match(
    parityCell("motion-thinking-shimmer", "pi"),
    /wall frame burst 40 x 100 ms/u,
  );
});

test("live Codex references name recorded frames when the recording is present", () => {
  const root = process.env.SAGE_CODEX_LIVE;
  if (!root) return;
  for (const { live } of PARITY_STATES)
    for (const ref of live) {
      const [segment, frame] = ref.split("/");
      const folder = readdirSync(root).find((name) =>
        name.startsWith(segment + "-"),
      );
      assert.ok(folder, ref);
      assert.ok(
        readdirSync(path.join(root, folder)).some((name) =>
          name.startsWith(frame + "-"),
        ),
        ref,
      );
    }
});

test("parity selectors stay anchored to the wave 6-8 product components", () => {
  const anchors = [
    [
      "features/chat/TranscriptSelectionPopover.tsx",
      [
        'className="chat-selection-popover"',
        'aria-label="Selection actions"',
        "Add to chat",
        "Ask in side chat",
      ],
    ],
    [
      "features/chat/AnnotationComment.tsx",
      [
        'className="annotation-comment"',
        'aria-label="Annotation comment"',
        'aria-label="Save annotation"',
      ],
    ],
    [
      "features/chat/TranscriptAnnotations.tsx",
      ['className="transcript-annotation-badge"'],
    ],
    [
      "features/chat/AnnotationChip.tsx",
      ["annotation-chip--${draft ?", 'className="composer__annotation-row"'],
    ],
    [
      "workbench/SideChatChrome.tsx",
      [
        "side-chat-tab",
        'aria-label="Close side chat"',
        'className="side-chat-home"',
        // w9b adds the shared dim scrim class after it.
        'className="close-side-chat-dialog',
      ],
    ],
    [
      "workbench/AgentChatCenterHeader.tsx",
      [
        "className={`agent-chat-center__header",
        '() => import("./AgentSideChatHeaders")',
        'part="AgentSideChatHeaders"',
      ],
    ],
    [
      "workbench/AgentSideChatHeaders.tsx",
      ['className="agent-chat-center__side-tab"', 'part="SideChatHeader"'],
    ],
    [
      "features/chat/TurnNavigator.tsx",
      ['className="turn-navigator"', "data-active={distance === 0}"],
    ],
    [
      "features/chat/TurnPreviewCard.tsx",
      ['className="turn-navigator__preview"', 'role="tooltip"'],
    ],
    [
      "features/chat/ChatFindBar.tsx",
      [
        'className="chat-find"',
        'role="search"',
        'aria-label="Search chat"',
        'role="status"',
      ],
    ],
    [
      "features/chat/acp/ToolItemDetails.tsx",
      ['className="acp-tool-error"', 'className="acp-tool-result"'],
    ],
    [
      "features/chat/acp/PermissionPrompt.tsx",
      [
        'className="acp-permission-prompt"',
        'className="acp-permission-prompt__actions"',
        'className="acp-permission-prompt__split"',
        'className="acp-permission-prompt__stop"',
        'className="acp-permission-prompt__question"',
      ],
    ],
    ["features/chat/useApprovalSlot.tsx", ['aria-label="Permission request"']],
    [
      "features/chat/acp/CollaborationItem.tsx",
      ['className="acp-subagent-name"', "data-state={state}"],
    ],
    [
      "workbench/SubagentSurface.tsx",
      ['className="subagent-surface__bar"', 'aria-label="Back to subagents"'],
    ],
    [
      "workbench/SubagentResourceRows.tsx",
      ["aria-label={`${title} subagents`}"],
    ],
    [
      "features/chat/ForkChat.tsx",
      [
        'aria-label="More actions"',
        'className="answer-more-menu"',
        'role="menuitem"',
        'className="fork-chat-dialog',
        'className="chat-fork-divider"',
      ],
    ],
    ["features/chat/ChatTranscript.tsx", ['className="chat-follow-end"']],
    ["features/chat/acp/TurnActivityItem.tsx", ["data-selectable-answer="]],
  ];
  for (const [file, needles] of anchors) {
    const text = source(file);
    for (const needle of needles)
      assert.ok(text.includes(needle), file + ": " + needle);
  }
  const css = (file) =>
    readFileSync(new URL("../src/" + file, import.meta.url), "utf8");
  assert.match(
    css("features/chat/acp/chat-markdown.css"),
    /@keyframes acp-stream-in/u,
  );
  assert.match(css("styles/global.css"), /@keyframes sage-row-in/u);
  assert.match(
    css("features/chat/acp/acp-activity-details.css"),
    /@keyframes acp-live-shimmer/u,
  );
  assert.match(
    css("features/chat/turn-navigator.css"),
    /@container chat \(width < 720px\)/u,
  );
});

test("parity replays reuse their base events and add only pinned wire shapes", () => {
  const cases = new Map(nativeCases().map((entry) => [entry.id, entry.steps]));
  const items = (id) =>
    cases
      .get(id)
      .filter((step) => step.params?.item)
      .map((step) => step.params.item);
  const mcp = items("codex-tool-mcp-result");
  assert.deepEqual(
    mcp.map(({ type, status }) => type + ":" + status),
    ["mcpToolCall:inProgress", "mcpToolCall:completed"],
  );
  assert.equal(mcp[1].result.structuredContent.hits.length, 1);
  assert.equal(
    items("codex-tool-mcp-error")[1].error.message,
    "Synthetic MCP probe error",
  );
  assert.deepEqual(
    [...new Set(items("codex-tool-mixed-group").map(({ type }) => type))],
    ["commandExecution", "webSearch", "mcpToolCall"],
  );
  const claudeWeb = cases
    .get("claude-tool-web-search")
    .find((step) => step.update?.kind === "fetch");
  assert.deepEqual(claudeWeb.update.rawInput, {
    query: "synthetic native replay",
  });
  for (const harness of ["codex", "claude"])
    for (const state of ["fork-menu", "fork-dialog", "fork-thread"]) {
      const steps = cases.get(harness + "-" + state);
      const fork = steps.findIndex(
        ({ kind }) => kind === "codexFork" || kind === "acpFork",
      );
      assert.equal(fork >= 0, state === "fork-thread", harness + "-" + state);
      if (harness === "claude")
        assert.equal(steps[1].shape, "acpInitializeFork");
      if (fork >= 0) {
        const settled = steps.findIndex(
          ({ shape }) => shape === "codexCompleted" || shape === "acpCompleted",
        );
        assert.ok(settled >= 0 && settled < fork);
      }
    }
  assert.equal(cases.get("claude-turn-completed")[1].shape, "acpInitialize");
  for (const harness of HARNESSES) {
    const prompts = (state) =>
      cases
        .get(harness + "-" + state)
        .filter(({ kind }) =>
          ["codexTurnStart", "acpPrompt", "piPrompt"].includes(kind),
        ).length;
    for (const state of PARITY_STATE_NAMES.filter((state) =>
      paritySupported(harness).includes(state),
    ))
      // Approvals answer a populated setup Turn before the approval Turn.
      assert.equal(
        prompts(state),
        parityTurns(state) + (state.startsWith("approval-") ? 1 : 0),
        harness + "-" + state,
      );
    const live = cases.get(harness + "-motion-stream-fade");
    assert.ok(
      live.some(({ kind }) =>
        ["codexInterrupt", "acpCancel", "piAbort"].includes(kind),
      ),
    );
  }
  // Named children: each spawn is followed by its own activity naming it.
  const named = items("codex-subagents-name-hover");
  assert.deepEqual(
    named.map(({ type, agentPath }) => type + ":" + (agentPath ?? "")),
    [
      "collabAgentToolCall:",
      "subAgentActivity:Count math",
      "collabAgentToolCall:",
      "subAgentActivity:Count readme",
    ],
  );
  // The shimmer replay holds a running tool live until Stop.
  for (const harness of HARNESSES) {
    const steps = cases.get(harness + "-motion-thinking-shimmer");
    assert.ok(
      steps.some(({ kind }) =>
        ["codexInterrupt", "acpCancel", "piAbort"].includes(kind),
      ),
      harness,
    );
  }
  assert.ok(
    items("codex-motion-thinking-shimmer").some(
      ({ type, status }) =>
        type === "commandExecution" && status === "inProgress",
    ),
  );
  // Pi discovers thinking levels once per activation, not per prompt.
  for (const state of ["annotation-sent", "turn-navigator-hover"]) {
    const kinds = cases
      .get("pi-" + state)
      .map(({ kind }) => kind)
      .filter(Boolean);
    const second = kinds.lastIndexOf("piPrompt");
    assert.ok(second > kinds.indexOf("piPrompt"), state);
    assert.ok(!kinds.slice(second).includes("piThinkingLevels"), state);
  }
  const subagents = items("codex-subagents-tab-active-done").filter(
    ({ type }) => type === "collabAgentToolCall",
  );
  assert.deepEqual(
    subagents.map(({ status }) => status),
    ["completed", "inProgress"],
  );
  // Each child names itself on the wire, as Codex's do (s4/f0244).
  assert.deepEqual(
    items("codex-subagents-tab-active-done")
      .filter(({ type }) => type === "subAgentActivity")
      .map(({ agentPath, kind }) => agentPath + ":" + kind),
    ["Count math:completed", "Count readme:started"],
  );
  // Codex worked-expand: three command rows, each after its own commentary
  // message, so the opened Worked group staggers sibling rows (s2/f0015).
  const workedOrder = (id) =>
    cases
      .get(id)
      .flatMap((step) =>
        step.params?.itemId
          ? ["text:" + step.params.itemId]
          : step.method === "item/completed" &&
              step.params.item.type === "commandExecution"
            ? ["row"]
            : [],
      );
  assert.deepEqual(workedOrder("codex-motion-worked-expand"), [
    "text:fixture-commentary-1",
    "row",
    "text:fixture-commentary-2",
    "row",
    "text:fixture-commentary-3",
    "row",
    "text:fixture-answer",
  ]);
  // Claude merges a Turn's prose into one message; it keeps the shared replay.
  assert.deepEqual(
    cases.get("claude-motion-worked-expand"),
    cases.get("claude-worked-expanded"),
  );
  assert.ok(
    cases
      .get("codex-subagents-tab-active-done")
      .some(({ kind }) => kind === "codexInterrupt"),
  );
});

const baseObservation = {
  chatId: "capture-chat",
  projectId: "project",
  projectionLoaded: true,
  singlePane: true,
  paneCount: 1,
  focusedPaneCount: 1,
  bindingId: HARNESS.codex.binding,
  selected: HARNESS.codex.id,
  turnId: "turn-2",
  turnCount: 1,
  state: "completed",
  assistantRendered: true,
  lazyFallbackVisible: false,
  itemKinds: [],
  toolStates: [],
  visible: {},
  controls: {},
};
const ready = {
  "side-chat-running": {},
  "selection-popover": { visible: { selectionPopover: true } },
  "annotation-field": {
    visible: {
      annotationComment: true,
      annotationBadge: true,
      draftChip: true,
    },
  },
  "annotation-chip": { visible: { annotationBadge: true, draftChip: true } },
  "annotation-sent": { turnCount: 2, visible: { sentChip: true } },
  "side-chat-open": {
    state: "none",
    visible: { sideChatTab: true, sideChatHome: true, draftChip: true },
  },
  "side-chat-close-confirm": {
    state: "none",
    visible: { sideChatTab: true, closeSideDialog: true },
  },
  "turn-navigator-hover": {
    turnCount: 2,
    visible: { navigatorActive: true, navigatorPreview: true },
  },
  "find-in-chat": { visible: { findBar: true }, findStatus: "1 / 3 results" },
  "tool-mcp-result": {
    toolStates: ["completed"],
    visible: { toolResult: true },
  },
  "tool-mcp-error": { toolStates: ["failed"], visible: { toolError: true } },
  "tool-web-search": { visible: { webSearch: true } },
  "tool-mixed-group": { visible: { mixedGroup: true } },
  "approval-card": {
    state: "waitingApproval",
    visible: { approvalPrompt: true },
  },
  "subagents-name-hover": {
    visible: { subagentName: true },
    hoveredSubagentName: 1,
  },
  "subagents-tab-active-done": {
    state: "running",
    visible: { subagentActive: true, subagentDone: true },
  },
  "subagent-child-header": { visible: { childHeader: true, childBack: true } },
  "fork-menu": { visible: { forkMenuItem: true } },
  "fork-dialog": { visible: { forkDialog: true } },
  "fork-thread": { visible: { forkDivider: true } },
  "motion-thinking-shimmer": {
    state: "running",
    visible: { rollup: true, shimmer: true },
  },
  "motion-stream-fade": { state: "running", streamAnimations: 4 },
  "motion-worked-expand": { visible: { workedClosed: true } },
  "motion-jump-pill": { visible: { jump: true } },
};
const owned = {
  side: {
    chatId: "side-1",
    sideParentChatId: "capture-chat",
    paneCount: 2,
    singlePane: false,
  },
  fork: { chatId: "fork-chat", forkedFrom: "capture-chat" },
};

test("each parity state has a positive and a missing-witness named state", () => {
  const captureChat = { chatId: "capture-chat", projectId: "project" };
  for (const state of PARITY_STATE_NAMES) {
    assert.ok(ready[state], state);
    const owner = state.startsWith("side-chat")
      ? owned.side
      : state === "fork-thread"
        ? owned.fork
        : {};
    const observation = { ...baseObservation, ...ready[state], ...owner };
    const predicates = namedState(
      { harness: "codex", state },
      observation,
      captureChat,
    );
    if (state !== "side-chat-running")
      assert.deepEqual(
        Object.values(predicates).every(Boolean),
        true,
        state + JSON.stringify(predicates),
      );
    assert.equal(
      namedState(
        { harness: "codex", state },
        { ...observation, visible: {}, streamAnimations: 0, findStatus: "" },
      ).namedState,
      false,
      state,
    );
    assert.equal(
      namedState(
        { harness: "codex", state },
        { ...observation, lazyFallbackVisible: true },
      ).namedState,
      false,
      state,
    );
  }
  const view = (visible, extra = {}) => ({
    ...baseObservation,
    ...extra,
    visible,
  });
  const rules = (observation) =>
    parityRules(observation, {
      terminal: observation.state === "completed",
      live: observation.state === "running",
      assistantRendered: true,
    });
  assert.equal(
    rules(
      view({ annotationComment: true, annotationBadge: true, draftChip: true }),
    )["annotation-chip"],
    false,
  );
  assert.equal(
    rules(view({ sentChip: true, annotationBadge: true }, { turnCount: 2 }))[
      "annotation-sent"
    ],
    false,
  );
  assert.equal(
    rules(view({ findBar: true }, { findStatus: "0 results" }))["find-in-chat"],
    false,
  );
  assert.equal(
    rules(view({ findBar: true }, { findStatus: "1 / 500+ results" }))[
      "find-in-chat"
    ],
    true,
  );
  assert.equal(
    rules(view({ subagentName: true }, { hoveredSubagentName: 0 }))[
      "subagents-name-hover"
    ],
    false,
  );
  assert.equal(
    rules(
      view({ approvalPrompt: true, composerInput: true }, { state: "running" }),
    )["approval-card"],
    false,
  );
  // Live reasoning prose never shimmers; only a live label (s8 "Running …").
  assert.equal(
    rules(view({ thought: true, shimmer: false }, { state: "running" }))[
      "motion-thinking-shimmer"
    ],
    false,
  );
  assert.equal(
    rules(view({ tool: true, shimmer: true }, { state: "running" }))[
      "motion-thinking-shimmer"
    ],
    true,
  );
  assert.equal(
    rules(view({ toolResult: true }, { toolStates: ["failed"] }))[
      "tool-mcp-result"
    ],
    false,
  );
});

test("subagents-tab-active-done clicks one name of the row naming both children", () => {
  // The replay renders one row with a name button per child (C1), and the
  // click admits exactly one match, so an unqualified name selector never
  // clicks and the capture times out.
  const children = nativeCases()
    .find(({ id }) => id === "codex-subagents-tab-active-done")
    .steps.filter(({ params }) => params?.item?.type === "subAgentActivity");
  assert.equal(children.length, 2);
  const [open] = parityStepsFor("subagents-tab-active-done");
  assert.equal(open.op, "click");
  assert.match(open.selector, /\.acp-subagent-name:first-of-type$/u);
});

test("web-search and mixed-group states are captured with the row opened", () => {
  for (const state of ["tool-web-search", "tool-mixed-group"])
    assert.deepEqual(
      parityStepsFor(state).map(({ selector }) => selector),
      [
        ".acp-turn:last-of-type .chat-transcript-work > summary",
        ".acp-turn:last-of-type .acp-tool-rollup > summary",
      ],
      state,
    );
  assert.match(PARITY_PANE_SELECTORS.webSearch, /\[open\]$/u);
  assert.match(
    PARITY_PANE_SELECTORS.mixedGroup,
    /\[open\] > \.acp-tool-rollup__items$/u,
  );
});

test("side and fork states own the Chat the drive opened from the capture Chat", () => {
  const captureChat = { chatId: "capture-chat", projectId: "project" };
  const side = { ...baseObservation, ...owned.side };
  assert.deepEqual(parityChatPredicates("side-chat-open", side, captureChat), {
    chat: true,
    singlePane: true,
  });
  assert.equal(
    parityChatPredicates(
      "side-chat-open",
      { ...side, sideParentChatId: "other" },
      captureChat,
    ).chat,
    false,
  );
  assert.equal(
    parityChatPredicates(
      "side-chat-open",
      { ...side, chatId: "capture-chat" },
      captureChat,
    ).chat,
    false,
  );
  assert.equal(
    parityChatPredicates(
      "side-chat-open",
      { ...side, paneCount: 3 },
      captureChat,
    ).singlePane,
    false,
  );
  const fork = { ...baseObservation, ...owned.fork };
  assert.equal(
    parityChatPredicates("fork-thread", fork, captureChat).chat,
    true,
  );
  assert.equal(
    parityChatPredicates(
      "fork-thread",
      { ...fork, forkedFrom: "other" },
      captureChat,
    ).chat,
    false,
  );
  assert.equal(
    parityChatPredicates(
      "fork-thread",
      { ...fork, chatId: "capture-chat" },
      captureChat,
    ).chat,
    false,
  );
  assert.equal(
    parityChatPredicates("find-in-chat", side, captureChat).chat,
    false,
  );
  assert.equal(
    parityChatPredicates("find-in-chat", baseObservation, captureChat).chat,
    true,
  );
});

async function observe(markup, outside = "", page = {}, sides = []) {
  const window = new JSDOM(
    `<section class="agent-chat-pane agent-chat-pane--focused">${markup}</section>${outside}`,
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
    window.document.getAnimations = () => [
      { animationName: "acp-stream-in" },
      { animationName: "other" },
    ];
    const chatId = page.chatId ?? "capture-chat";
    window.workbench = {
      useWorkbenchStore: {
        getState: () => ({
          activeTabId: chatId,
          project: { id: "project" },
          tabs: [{ id: chatId, kind: "chat" }],
        }),
      },
    };
    window.registry = {
      acpProjectionStoreFor: () => ({
        getState: () => ({
          page: {
            chatId,
            projectId: "project",
            turns: [
              {
                id: "turn",
                sessionId: "session",
                state: "completed",
                items: [],
              },
            ],
            sessions: [{ id: "session", bindingId: HARNESS.codex.binding }],
            ...page,
          },
        }),
      }),
    };
    window.owner = { pinnedAgentChatOwner: () => chatId };
    window.sides = { useSideChatState: { getState: () => ({ sides }) } };
    return await window.eval(
      observationFor(HARNESS.codex.binding)
        .replace("await import('/src/app/store.ts')", "workbench")
        .replace(
          "await import('/src/app/acpProjectionRegistry.ts')",
          "registry",
        )
        .replace("await import('/src/app/agentChatOwner.ts')", "owner")
        .replace(
          "await import('/src/workbench/sideChatState.ts')",
          "await Promise.resolve(sides)",
        ),
    );
  } finally {
    window.close();
  }
}

test("observation sees body-portaled parity surfaces and center-header side chat chrome", async () => {
  const observation = await observe(
    '<div class="side-chat-home">Side chat</div><div class="composer__annotation-row"><div class="annotation-chip annotation-chip--draft"><button>1 selection</button></div></div><section class="chat-find" role="search"><div><span role="status">1 / 3 results</span></div></section>',
    '<div class="agent-chat-center__header"><div class="agent-chat-center__side-tab"><div class="agent-chat-pane__identity side-chat-tab"><strong>Side chat</strong><button aria-label="Close side chat"></button></div></div></div><div class="chat-selection-popover" role="toolbar"><button>Add to chat</button><button>Ask in side chat</button></div><button class="transcript-annotation-badge">1</button><dialog class="close-side-chat-dialog" open></dialog>',
    { chatId: "side-1", turns: [] },
    [{ id: "side-1", parentChatId: "capture-chat" }],
  );
  for (const kind of [
    "sideChatTab",
    "sideChatHome",
    "draftChip",
    "findBar",
    "selectionPopover",
    "annotationBadge",
    "closeSideDialog",
  ])
    assert.equal(observation.visible[kind], true, kind);
  assert.equal(observation.visible.forkDialog, false);
  // The tab is witnessed only in the Agent center header row (w9int).
  const stray = await observe(
    '<div class="agent-chat-pane__identity side-chat-tab">Side chat</div>',
  );
  assert.equal(stray.visible.sideChatTab, false);
  const misplaced = await observe(
    "",
    '<div class="agent-chat-center__side-tab"><div class="side-chat-tab">Side chat</div></div>',
  );
  assert.equal(misplaced.visible.sideChatTab, false);
  const close = parityStepsFor("side-chat-close-confirm").at(-1);
  assert.equal(close.scope, "document");
  assert.equal(
    new JSDOM(
      '<div class="agent-chat-center__header"><div class="agent-chat-center__side-tab"><div class="side-chat-tab"><button aria-label="Close side chat"></button></div></div></div>',
    ).window.document.querySelectorAll(close.selector).length,
    1,
  );
  assert.equal(observation.sideParentChatId, "capture-chat");
  assert.equal(observation.findStatus, "1 / 3 results");
  assert.equal(observation.streamAnimations, 1);
  assert.equal(observation.paneCount, 1);
  assert.equal(
    namedState(
      { harness: "codex", state: "side-chat-close-confirm" },
      observation,
      { chatId: "capture-chat", projectId: "project" },
    ).namedState,
    true,
  );
  const fork = await observe(
    '<div class="chat-fork-divider">Continued from chat</div>',
    "",
    { chatId: "fork-chat", forkedFrom: "capture-chat", turns: [] },
  );
  assert.equal(fork.forkedFrom, "capture-chat");
  assert.equal(fork.visible.forkDivider, true);
  // Turn-scoped tool witnesses from an older Turn never count.
  const stale = await observe(
    '<section class="acp-turn" data-turn-id="older"><div class="acp-tool-error" role="alert">x</div></section>',
  );
  assert.equal(stale.visible.toolError, false);
  const current = await observe(
    '<section class="acp-turn" data-turn-id="turn"><div class="acp-tool-error" role="alert">x</div></section>',
  );
  assert.equal(current.visible.toolError, true);
  for (const selectors of [PARITY_PANE_SELECTORS, PARITY_DOCUMENT_SELECTORS])
    for (const selector of Object.values(selectors))
      new JSDOM("").window.document.querySelectorAll(selector);
});

test("parity steps pin Turn selectors and drive with real pointer and key input", async () => {
  for (const state of PARITY_STATE_NAMES)
    for (const step of parityStepsFor(state)) {
      assert.ok(
        [
          "settle",
          "submit",
          "open",
          "click",
          "fill",
          "select",
          "hover",
          "key",
          "forked",
        ].includes(step.op),
        state,
      );
      if (step.selector?.startsWith(".acp-turn:last-of-type "))
        assert.match(
          scopeToTurn(step.selector, "turn-1"),
          /^\.acp-turn\[data-turn-id="turn-1"\] /u,
        );
    }
  assert.equal(parityStepsFor("approval-card").length, 0);
  assert.equal(
    parityStepsFor("annotation-sent").filter(({ op }) => op === "submit")
      .length,
    1,
  );
  assert.throws(
    () => phraseRectExpression("bad id", SELECTED_PHRASE),
    /turn-identity/u,
  );
  assert.throws(
    () => fillExpression("input", "document", "x".repeat(257)),
    /parity-fill/u,
  );
  assert.throws(
    () => scopedClickExpression("button", "window"),
    /parity-scope/u,
  );
  assert.match(
    centerExpression("button", "owner", "capture-chat"),
    /"capture-chat"/u,
  );
  const mouse = [],
    keys = [],
    evaluated = [];
  let observation = { ...baseObservation, turnId: "turn-1" };
  const evaluate = async (expression) => {
    evaluated.push(expression);
    if (expression.includes("createTreeWalker"))
      return { x: 10, y: 20, width: 100, height: 16 };
    if (expression.includes("box.x+box.width/2")) return { x: 50, y: 60 };
    return true;
  };
  const cdp = {
    mouse: async (event) => mouse.push(event),
    key: async (event) => keys.push(event),
  };
  const run = (state, submit = async () => "turn-2") =>
    driveParitySteps({
      cdp,
      evaluate,
      observe: async () => observation,
      state,
      captureChat: { chatId: "capture-chat", projectId: "project" },
      submittedTurnId: "turn-1",
      submit: async () => {
        const id = await submit();
        observation = { ...observation, turnId: id };
        return id;
      },
      deadline: Date.now() + 2000,
      record: () => {},
    });
  await run("selection-popover");
  assert.deepEqual(
    mouse.map(({ type }) => type),
    [
      "mouseMoved",
      "mousePressed",
      ...Array(8).fill("mouseMoved"),
      "mouseReleased",
    ],
  );
  assert.equal(mouse[1].x, 11);
  assert.equal(mouse.at(-1).x, 111);
  for (const event of mouse) mouseParams(event);
  mouse.length = 0;
  assert.equal(await run("turn-navigator-hover"), "turn-2");
  assert.deepEqual(mouse.at(-1), { type: "mouseMoved", x: 50, y: 60 });
  observation = { ...baseObservation, turnId: "turn-1" };
  await run("find-in-chat");
  assert.deepEqual(keys, [
    { type: "rawKeyDown", key: "f", ctrl: true },
    { type: "keyUp", key: "f", ctrl: true },
  ]);
  for (const event of keys) assert.equal(keyParams(event).modifiers, 2);
  assert.ok(evaluated.some((expression) => expression.includes('"synthetic"')));
  evaluated.length = 0;
  observation = { ...baseObservation, turnId: "turn-1" };
  await run("subagents-tab-active-done");
  assert.ok(
    evaluated.some((expression) =>
      expression.includes(
        '.acp-turn[data-turn-id=\\"turn-1\\"] .acp-subagent-row .acp-subagent-name:first-of-type',
      ),
    ),
  );
  assert.ok(
    evaluated.some(
      (expression) =>
        expression.includes("Back to subagents") &&
        expression.includes('"capture-chat"'),
    ),
  );
});

test("parity settle waits out the lazy transcript and Ctrl+F retries until the bar opens", async () => {
  const keys = [];
  let observation = {
    ...baseObservation,
    turnId: "turn-1",
    lazyFallbackVisible: true,
  };
  let observed = 0;
  const run = driveParitySteps({
    cdp: { mouse: async () => {}, key: async (event) => keys.push(event) },
    evaluate: async (expression) => {
      // The find bar appears only after the second press.
      if (expression.endsWith(".length===1)()")) return keys.length >= 4;
      return true;
    },
    observe: async () => {
      // The transcript mounts after a few polls.
      if (++observed === 3)
        observation = { ...observation, lazyFallbackVisible: false };
      return observation;
    },
    state: "find-in-chat",
    captureChat: { chatId: "capture-chat", projectId: "project" },
    submittedTurnId: "turn-1",
    submit: async () => "turn-2",
    deadline: Date.now() + 10000,
    record: () => {},
  });
  await run;
  assert.ok(observed >= 3);
  assert.deepEqual(
    keys.map(({ type }) => type),
    ["rawKeyDown", "keyUp", "rawKeyDown", "keyUp"],
  );
  assert.match(
    presentExpression('.chat-find input[aria-label="Search chat"]'),
    /Search chat/u,
  );
  assert.equal(
    parityStepsFor("find-in-chat").find(({ op }) => op === "key").expect,
    '.chat-find input[aria-label="Search chat"]',
  );
});

test("fork-thread observes the fork Chat only after its reload fallback passes", async () => {
  assert.equal(parityStepsFor("fork-thread").at(-1).op, "forked");
  // Native trace: fork rendered, a ~300 ms lazy fallback, rendered again.
  const fork = {
    ...baseObservation,
    chatId: "fork-chat",
    forkedFrom: "capture-chat",
  };
  const timeline = [
    { ...baseObservation, lazyFallbackVisible: true },
    fork,
    { ...fork, lazyFallbackVisible: true },
    { ...fork, lazyFallbackVisible: true },
  ];
  let polls = 0;
  const started = Date.now();
  await driveParitySteps({
    cdp: { mouse: async () => {}, key: async () => {} },
    evaluate: async () => true,
    observe: async () => ({ ...(timeline[polls++] ?? fork), turnId: "turn-1" }),
    state: "fork-thread",
    captureChat: { chatId: "capture-chat", projectId: "project" },
    submittedTurnId: "turn-1",
    submit: async () => "turn-2",
    deadline: Date.now() + 10000,
    record: () => {},
  });
  assert.ok(Date.now() - started >= FORK_QUIET_MS);
  assert.ok(polls > timeline.length);
});

test("CDP input shapes are closed", () => {
  assert.deepEqual(
    mouseParams({ type: "mousePressed", x: 1, y: 2, button: "left" }),
    {
      type: "mousePressed",
      x: 1,
      y: 2,
      button: "left",
      buttons: 1,
      clickCount: 1,
    },
  );
  for (const bad of [
    { type: "mouseWheel", x: 1, y: 1 },
    { type: "mouseMoved", x: -1, y: 1 },
    { type: "mouseMoved", x: 1, y: 1e6 },
    { type: "mousePressed", x: 1, y: 1 },
    { type: "mouseMoved", x: 1, y: 1, button: "right" },
  ])
    assert.throws(() => mouseParams(bad), /journey-cdp-input-invalid/u);
  for (const bad of [
    { type: "char", key: "f" },
    { type: "keyUp", key: "F" },
    { type: "keyUp", key: "ff" },
    { type: "keyUp", key: "f", ctrl: 1 },
  ])
    assert.throws(() => keyParams(bad), /journey-cdp-input-invalid/u);
});

test("motion bursts sample wall-clock or seeked animation frames within bounds", async () => {
  for (const state of PARITY_STATE_NAMES.filter(parityMotion))
    validateBurstSpec(parityMotion(state));
  assert.throws(
    () => validateBurstSpec({ mode: "wall", frames: 49, intervalMs: 50 }),
    /burst-spec/u,
  );
  assert.throws(
    () => validateBurstSpec({ mode: "seek", frames: 4, intervalMs: 50 }),
    /burst-spec/u,
  );
  assert.throws(() => seekExpression(["bad name"]), /burst-spec/u);
  assert.match(
    seekExpression(["acp-stream-in"], 150),
    /animation\.currentTime=150;/u,
  );
  assert.doesNotMatch(seekExpression(["acp-stream-in"]), /currentTime=/u);
  const png = Buffer.from("png");
  let shots = 0;
  const cdp = { screenshot: async () => (shots++, png.toString("base64")) };
  const seeks = [];
  const seek = await captureBurst({
    cdp,
    evaluate: async (expression) => (seeks.push(expression), 3),
    spec: parityMotion("motion-worked-expand"),
    turnId: "turn-1",
    deadline: Date.now() + 5000,
    record: () => {},
  });
  assert.equal(seek.frames.length, 11);
  assert.deepEqual(
    seek.frames.map(({ animationMs }) => animationMs),
    Array.from({ length: 11 }, (_, index) => index * 60),
  );
  assert.ok(
    seeks[0].includes(
      '.acp-turn[data-turn-id=\\"turn-1\\"] .chat-transcript-work > summary',
    ),
  );
  const wall = await captureBurst({
    cdp,
    evaluate: async () => true,
    spec: { mode: "wall", frames: 4, intervalMs: 20 },
    turnId: "turn-1",
    deadline: Date.now() + 5000,
    record: () => {},
  });
  assert.equal(wall.frames.length, 4);
  for (let index = 1; index < 4; index++)
    assert.ok(wall.frames[index].atMs >= index * 20 - 2);
  assert.equal(shots, 15);
  await assert.rejects(
    captureBurst({
      cdp,
      evaluate: async (expression) =>
        expression.includes("currentTime") ? 0 : 1,
      spec: parityMotion("motion-stream-fade"),
      turnId: "turn-1",
      deadline: Date.now() + 2000,
      record: () => {},
    }),
    /burst-animation-lost/u,
  );
});

test("a seek holds only unfinished animations, so finished blocks are never rewound", async () => {
  const pane = {
    closest: (selector) => selector === ".agent-chat-pane--focused",
  };
  const animation = (playState, currentTime) => ({
    animationName: "acp-stream-in",
    playState,
    currentTime,
    effect: { target: pane },
    paused: 0,
    pause() {
      this.paused++;
      this.playState = "paused";
    },
  });
  const finished = animation("finished", 300);
  const running = animation("running", 40);
  const document = { getAnimations: () => [finished, running] };
  const run = (atMs) =>
    Function(
      "document",
      "requestAnimationFrame",
      `return ${seekExpression(["acp-stream-in"], atMs)}`,
    )(document, (callback) => setImmediate(callback));
  assert.equal(await run(null), 1);
  assert.equal(running.paused, 1);
  // A held animation stays held (paused) across later seeks.
  assert.equal(await run(120), 1);
  assert.equal(running.currentTime, 120);
  assert.equal(finished.paused, 0);
  assert.equal(finished.currentTime, 300);
  assert.equal(finished.playState, "finished");
});

test("burst frames are named rec.ps1-style beside the still with a hashed manifest", () => {
  const root = mkdtempSync(path.join(process.cwd(), ".nc4-burst-test-"));
  try {
    const png = Buffer.alloc(24);
    Buffer.from("89504e470d0a1a0a", "hex").copy(png);
    png.write("IHDR", 12);
    png.writeUInt32BE(720, 16);
    png.writeUInt32BE(1000, 20);
    const options = parseArgs([
      "--harness",
      "pi",
      "--state",
      "motion-stream-fade",
      "--width",
      "720",
    ]);
    const still = path.join(root, "motion-stream-fade-pi-720.png");
    const burst = burstManifest(still, options, {
      mode: "seek",
      intervalMs: 50,
      frames: [0, 1].map((index) => ({
        index,
        atMs: index * 7,
        animationMs: index * 50,
        png,
      })),
    });
    assert.deepEqual(
      burst.files.map(({ file }) => path.basename(file)),
      [
        "motion-stream-fade-pi-720-f01-000000ms.png",
        "motion-stream-fade-pi-720-f02-000050ms.png",
      ],
    );
    const manifest = JSON.parse(burst.json);
    assert.equal(manifest.caseId, "pi-motion-stream-fade");
    assert.match(manifest.frames[0].sha256, /^[a-f0-9]{64}$/u);
    assert.equal(
      path.basename(burst.path),
      "motion-stream-fade-pi-720.frames.json",
    );
    assert.throws(
      () =>
        burstManifest(still, options, {
          mode: "seek",
          intervalMs: 50,
          frames: [{ index: 0, atMs: 0, animationMs: 0, png }],
        }),
      /burst-manifest/u,
    );
    assert.throws(
      () =>
        burstManifest(still, options, {
          mode: "wall",
          intervalMs: 50,
          frames: [0, 1].map((index) => ({
            index,
            atMs: index,
            animationMs: null,
            png: Buffer.from("x"),
          })),
        }),
      /png-viewport/u,
    );
  } finally {
    rmSync(root, { recursive: true });
  }
});
