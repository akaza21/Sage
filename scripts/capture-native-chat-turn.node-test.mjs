import assert from "node:assert/strict";
import test from "node:test";
import vm from "node:vm";
import { readFileSync, rmSync } from "node:fs";
import path from "node:path";
import * as dom from "./capture-native-chat-dom.mjs";
import { paneSelector } from "./capture-native-chat-pane.mjs";
import { parseArgs } from "./capture-native-chat-contract.mjs";
import {
  nativeCases,
  truthfulAbsentStates,
  nativeCaptureProductGaps,
} from "./acp-native-cases.mjs";

const ready = {
  turnId: "new-turn",
  bindingId: "codex-app-server",
  state: "running",
  controls: { stop: true },
  assistantRendered: true,
  lazyFallbackVisible: false,
};

test("terminal capture waits for rendered content and rejects a visible lazy fallback", () => {
  for (const harness of ["codex", "claude", "pi"]) {
    const options = { harness, state: "turn-completed" };
    const observation = {
      ...ready,
      state: "completed",
      visible: {},
      itemKinds: [],
    };
    for (const missing of [
      { assistantRendered: false },
      { lazyFallbackVisible: true },
      { lazyFallbackVisible: undefined },
    ])
      assert.equal(
        dom.namedState(options, { ...observation, ...missing }).namedState,
        false,
        harness + ": " + JSON.stringify(missing),
      );
    assert.equal(dom.namedState(options, observation).namedState, true);
  }
});

test("all terminal predicates need rendered content and no chat lazy fallback", () => {
  const cases = [
    ["turn-completed", {}],
    ["worked-expanded", { worked: true }],
    ["tool-finished", { tool: true }],
    ["tools-grouped", { rollup: true }],
    ["tool-long-label", { tool: true }],
    ["files-pill", { files: true }],
    ["file-hunk", { hunk: true }],
    ["file-diff-tab", { diffTab: true, diffView: true }],
    ["git-finished", { tool: true }],
    ["subagents-finished", { subagent: true }],
    ["subagents-grouped", { subagent: true }],
    [
      "subagents-popover",
      { popover: true, subagentCounts: true, subagentEntries: true },
    ],
    ["plan-completed", { plan: true }],
    ["compaction-completed", { compaction: true }],
    ["compaction-failed", { compaction: true }],
    ["compaction-cancelled", { compaction: true }],
    ["cancelled-dialog", { notice: true }],
    ["approval-approved", { permission: true }],
    ["approval-declined", { permission: true }],
    ["turn-error", { failure: true }, "failed"],
    ["turn-stopped", { stopped: true }, "cancelled"],
    ["turn-stopped", { stopped: true }, "interrupted"],
    ["non-read-denial", { rollup: true }, "interrupted"],
    ["turn-retry", { answer: true }],
    ["answer-table-lists", { table: true }],
    ["answer-code", { code: true }],
    ["answer-mermaid", { mermaid: true }],
    ["answer-image", { image: true }],
    ["effort-menu", { effortMenu: true, effortSlider: true }],
    ["responsive-wide", {}],
    ["responsive-720", {}],
    ["responsive-480", {}],
    ["compaction-started", { compaction: true }, "running"],
  ];
  for (const [state, visible, turnState = "completed"] of cases) {
    const options = { harness: "pi", state };
    const observation = {
      ...ready,
      state: turnState,
      visible,
      itemKinds: [],
      toolStates: ["completed", "completed", "declined"],
      compactionStates: ["started", "completed", "failed", "cancelled"],
      permissionStates: ["resolvedAllowed", "resolvedDeclined"],
      noticeKinds: ["pi_dialog_cancelled"],
      controls: { send: true },
      turnCount: 1,
      failedTurns: 0,
      elements: [{ kind: "answer", text: "Synthetic retry completed." }],
      codeHighlighted: true,
      imageLoaded: true,
    };
    assert.equal(dom.namedState(options, observation).namedState, true, state);
    assert.equal(
      dom.namedState(options, { ...observation, lazyFallbackVisible: true })
        .namedState,
      false,
      state,
    );
    assert.equal(
      dom.namedState(options, {
        ...observation,
        assistantRendered: false,
        visible: {},
      }).namedState,
      false,
      state,
    );
  }
  for (const harness of ["codex", "claude"])
    for (const assistantRendered of [false, true]) {
      const observation = {
        ...ready,
        state: "completed",
        visible: {},
        itemKinds: [],
        turnCount: 2,
        failedTurns: 1,
        assistantRendered,
      };
      const options = { harness, state: "turn-retry" };
      assert.equal(
        dom.namedState(options, observation).namedState,
        assistantRendered,
      );
      assert.equal(
        dom.namedState(options, { ...observation, lazyFallbackVisible: true })
          .namedState,
        false,
      );
    }
  for (const [state, turnState, visible] of [
    ["turn-error", "failed", { failure: true }],
    ["turn-stopped", "cancelled", { stopped: true }],
    ["plan-completed", "completed", { plan: true }],
    ["compaction-completed", "completed", { compaction: true }],
  ])
    assert.equal(
      dom.namedState(
        { harness: "pi", state },
        {
          ...ready,
          state: turnState,
          assistantRendered: false,
          visible,
          itemKinds: [],
          compactionStates: ["completed"],
        },
      ).namedState,
      true,
      state,
    );
});

test("answer-code capture rejects a visible plain panel until coloured tokens render", () => {
  const observation = {
    ...ready,
    state: "completed",
    visible: { code: true },
    controls: { stop: false },
  };
  const options = { harness: "codex", state: "answer-code" };
  assert.equal(dom.namedState(options, observation).namedState, false);
  assert.equal(
    dom.namedState(options, { ...observation, codeHighlighted: false })
      .namedState,
    false,
  );
  assert.equal(
    dom.namedState(options, { ...observation, codeHighlighted: true })
      .namedState,
    true,
  );
});

test("answer-image capture rejects an unloaded or invisible witness", () => {
  const observation = {
    ...ready,
    state: "completed",
    visible: { image: true },
    controls: { stop: false },
  };
  const options = { harness: "claude", state: "answer-image" };
  assert.equal(dom.namedState(options, observation).namedState, false);
  assert.equal(
    dom.namedState(options, { ...observation, imageLoaded: false }).namedState,
    false,
  );
  assert.equal(
    dom.namedState(options, { ...observation, imageLoaded: true }).namedState,
    true,
  );
  assert.equal(
    dom.namedState(options, {
      ...observation,
      imageLoaded: true,
      visible: { image: false },
    }).namedState,
    false,
  );
});

test("jump capture waits for owned completion and overflowing rendered transcript", async () => {
  const { scrollCaptureTranscript } =
    await import("./capture-native-chat-drive.mjs");
  const { SCROLL_TO_START } = await import("./capture-native-chat-pane.mjs");
  let observations = 0;
  let scrolls = 0;
  await scrollCaptureTranscript(
    async (expression) => {
      if (expression === "observation()") {
        observations++;
        return {
          ...ready,
          state: observations === 1 ? "running" : "completed",
          visible: { jump: scrolls >= 2 },
        };
      }
      assert.equal(expression, SCROLL_TO_START);
      scrolls++;
      return scrolls >= 2;
    },
    "observation()",
    "new-turn",
    "codex-app-server",
    Date.now() + 1000,
  );
  assert.equal(scrolls, 2);
});

test("Cancel waits through queued/start/text races and writes bounded step timestamps", async () => {
  const { clickReadyTurn, captureTiming } =
    await import("./capture-native-chat-drive.mjs");
  const observations = [
    { ...ready, state: "queued", assistantRendered: false },
    { ...ready, assistantRendered: false },
    ready,
    ready,
  ];
  let clicked = 0;
  const context = vm.createContext({
    observation: () => observations.shift() ?? ready,
    document: {
      querySelectorAll: () => [
        {
          disabled: false,
          scrollIntoView() {},
          click() {
            clicked++;
          },
        },
      ],
    },
  });
  const timing = captureTiming();
  await clickReadyTurn(
    (expression) => vm.runInContext(expression, context),
    "observation()",
    'button[aria-label="Cancel turn"]',
    "new-turn",
    "codex-app-server",
    Date.now() + 1000,
    timing.record,
  );
  assert.equal(clicked, 1);
  assert.deepEqual(
    timing.steps.map(({ step }) => step),
    [
      "turn-readiness-start",
      "turn-ready",
      "turn-action-start",
      "turn-action-clicked",
    ],
  );
  for (const step of timing.steps) {
    assert.ok(Number.isSafeInteger(step.atMs));
    assert.equal(step.turnId, "new-turn");
  }
  assert.equal(timing.steps[0].assistantRendered, false);
  assert.equal(timing.steps[1].assistantRendered, true);
  for (const missing of [
    { controls: { stop: false } },
    { assistantRendered: false },
  ]) {
    const blockedTiming = captureTiming();
    await assert.rejects(
      clickReadyTurn(
        (expression) =>
          vm.runInNewContext(expression, {
            observation: () => ({ ...ready, ...missing }),
          }),
        "observation()",
        'button[aria-label="Cancel turn"]',
        "new-turn",
        "codex-app-server",
        Date.now() + 10,
        blockedTiming.record,
      ),
      /turn-readiness-timeout/u,
    );
    assert.equal(blockedTiming.steps.at(-1).step, "turn-action-blocked");
  }
});

test("live action readiness belongs to the submitted turn and binding", () => {
  assert.equal(dom.liveTurnReady(ready, "new-turn", "codex-app-server"), true);
  for (const changed of [
    { turnId: "old-turn" },
    { bindingId: "claude-agent-acp" },
    { state: "queued" },
    { state: "completed" },
    { controls: { stop: false } },
    { assistantRendered: false },
  ])
    assert.equal(
      dom.liveTurnReady(
        { ...ready, ...changed },
        "new-turn",
        "codex-app-server",
      ),
      false,
    );
  for (const state of dom.LIVE_TURN_STATES)
    assert.equal(
      dom.liveTurnReady({ ...ready, state }, "new-turn", "codex-app-server"),
      true,
    );
});

test("in-turn approval actions share the readiness gate and target the submitted turn", () => {
  for (const state of [
    "approval-approved",
    "approval-declined",
    "turn-stopped",
  ])
    assert.equal(dom.isLiveTurnAction(dom.actionsFor(state)[0]), true);
  assert.equal(
    dom.isLiveTurnAction(dom.actionsFor("worked-expanded")[0]),
    false,
  );
  assert.equal(
    dom.scopeToTurn(dom.actionsFor("approval-approved")[0], "new-turn"),
    '.acp-turn[data-turn-id="new-turn"] .acp-permission__actions button:first-child',
  );
  const label = { textContent: "Inspect synthetic example", shown: true };
  const document = {
    querySelectorAll: () => [
      { dataset: { turnId: "welcome" }, querySelectorAll: () => [label] },
      { dataset: { turnId: "new-turn" }, querySelectorAll: () => [] },
    ],
  };
  assert.equal(
    dom.assistantTextRendered("new-turn", document, (element) => element.shown),
    false,
  );
  document.querySelectorAll = () => [
    { dataset: { turnId: "new-turn" }, querySelectorAll: () => [label] },
  ];
  assert.equal(
    dom.assistantTextRendered("new-turn", document, (element) => element.shown),
    true,
  );
  label.shown = false;
  assert.equal(
    dom.assistantTextRendered("new-turn", document, (element) => element.shown),
    false,
  );
});

test("host-authored permission headers do not count as assistant text", () => {
  const header = { textContent: "Permission request" };
  const summary = { textContent: "" };
  const pending = {
    dataset: { turnId: "new-turn" },
    querySelectorAll: (selector) =>
      selector.includes("header > strong") ? [header] : [summary],
  };
  const document = { querySelectorAll: () => [pending] };
  assert.equal(
    dom.assistantTextRendered("new-turn", document, () => true),
    false,
  );
  summary.textContent = "Inspect synthetic example";
  assert.equal(
    dom.assistantTextRendered("new-turn", document, () => true),
    true,
  );
});

test("submission waits for a new owned Turn, not a count-only update", async () => {
  const { submitTurn, captureTiming } =
    await import("./capture-native-chat-drive.mjs");
  const samples = [
    { ...ready, turnId: "previous", turnCount: 1 },
    { ...ready, turnId: "previous", turnCount: 2 },
    { ...ready, bindingId: "other", turnCount: 2 },
    { ...ready, turnCount: 2 },
  ];
  const timing = captureTiming();
  const evaluate = (expression) =>
    expression === "observation()" ? samples.shift() : true;
  assert.equal(
    await submitTurn(
      evaluate,
      "observation()",
      async () => {},
      "codex-app-server",
      Date.now() + 1000,
      timing.record,
    ),
    "new-turn",
  );
  assert.equal(timing.steps.at(-1).step, "turn-observed");
});
test("model capture waits through sending, live turns and disabled composer controls", async () => {
  const { waitForComposerIdle, captureTiming } =
    await import("./capture-native-chat-drive.mjs");
  const samples = [
    { ...ready, state: "sending" },
    ready,
    {
      ...ready,
      state: "completed",
      controls: { stop: true, send: true, modelEnabled: true },
    },
    {
      ...ready,
      state: "completed",
      controls: { stop: false, send: true, modelEnabled: false },
    },
    {
      ...ready,
      state: "completed",
      controls: { stop: false, send: true, modelEnabled: true },
    },
  ];
  const timing = captureTiming();
  const idle = await waitForComposerIdle(
    async () => samples.shift(),
    "observation()",
    "new-turn",
    "codex-app-server",
    Date.now() + 1000,
    timing.record,
  );
  assert.equal(samples.length, 0);
  assert.equal(idle.state, "completed");
  assert.deepEqual(
    timing.steps.map(({ step }) => step),
    ["composer-idle-start", "composer-idle-ready"],
  );
});
test("model capture rejects another turn and a model list that never hydrates", async () => {
  const { waitForComposerIdle, captureTiming } =
    await import("./capture-native-chat-drive.mjs");
  for (const changed of [{ turnId: "foreign" }, { bindingId: "foreign" }])
    await assert.rejects(
      waitForComposerIdle(
        async () => ({ ...ready, ...changed }),
        "observation()",
        "new-turn",
        "codex-app-server",
        Date.now() + 1000,
        captureTiming().record,
      ),
      /turn-identity-drift/u,
    );
  await assert.rejects(
    waitForComposerIdle(
      async () => ({
        ...ready,
        state: "completed",
        controls: { stop: false, send: true, modelEnabled: false },
      }),
      "observation()",
      "new-turn",
      "codex-app-server",
      Date.now() + 25,
      captureTiming().record,
    ),
    /state-timeout/u,
  );
});

test("cleanup failures get timestamps and do not skip remaining owned cleanup", async () => {
  const { cleanupCapture, captureTiming } =
    await import("./capture-native-chat-drive.mjs");
  const timing = captureTiming();
  let completed = false;
  const failure = await cleanupCapture(timing.record, [
    {
      step: "app-stop",
      action: async () => {
        throw new Error("owned-cleanup-timeout");
      },
    },
    {
      step: "vite-stop",
      action: async () => {
        completed = true;
      },
    },
  ]);
  assert.equal(failure.message, "owned-cleanup-timeout");
  assert.equal(completed, true);
  assert.deepEqual(
    timing.steps.map((entry) => entry.step),
    [
      "app-stop-start",
      "app-stop-failed",
      "vite-stop-start",
      "vite-stop-finished",
    ],
  );
  let cleaned = false;
  const bounded = await cleanupCapture(() => {
    throw new Error("timing-bound");
  }, [
    {
      step: "app-stop",
      action: async () => {
        cleaned = true;
      },
    },
  ]);
  assert.equal(bounded.message, "timing-bound");
  assert.equal(cleaned, true);
  const full = captureTiming();
  for (let index = 0; index < 64; index++) full.record("prior-step");
  let closed = false;
  const closeError = await cleanupCapture(full.record, [
    {
      step: "cdp-close",
      action: async () => {
        closed = true;
      },
    },
  ]);
  assert.equal(closeError.message, "timing-bound");
  assert.equal(closed, true);
  assert.equal(full.steps.length, 64);
});

test("approval click revalidates live readiness and negotiated enabled controls", async () => {
  let clicked = 0;
  const action = dom.scopeToTurn(
    dom.actionsFor("approval-approved")[0],
    "new-turn",
  );
  const control = {
    disabled: true,
    scrollIntoView() {},
    click() {
      clicked++;
    },
  };
  const expression = dom.liveActionExpression(
    JSON.stringify({ ...ready, state: "waitingApproval" }),
    action,
    "new-turn",
    "codex-app-server",
  );
  const document = {
    querySelectorAll: (selector) =>
      selector === paneSelector(action) ? [control] : [],
  };
  assert.equal(await vm.runInNewContext(expression, { document }), false);
  control.disabled = false;
  assert.equal(await vm.runInNewContext(expression, { document }), true);
  assert.equal(clicked, 1);
  document.querySelectorAll = () => [];
  assert.equal(await vm.runInNewContext(expression, { document }), false);
});

test("a Turn settling between readiness and Cancel is blocked without a click", async () => {
  const { clickReadyTurn, captureTiming } =
    await import("./capture-native-chat-drive.mjs");
  const completed = { ...ready, state: "completed", controls: { stop: false } };
  const samples = [ready, ready, completed, completed];
  let clicked = false;
  const context = vm.createContext({
    observation: () => samples.shift() ?? completed,
    document: {
      querySelectorAll: () => [
        {
          disabled: false,
          scrollIntoView() {},
          click() {
            clicked = true;
          },
        },
      ],
    },
  });
  const timing = captureTiming();
  await assert.rejects(
    clickReadyTurn(
      async (expression) => {
        try {
          return await vm.runInContext(expression, context);
        } catch (error) {
          throw new Error("evaluate-invalid", { cause: error });
        }
      },
      "observation()",
      'button[aria-label="Cancel turn"]',
      "new-turn",
      "codex-app-server",
      Date.now() + 1000,
      timing.record,
    ),
    /turn-action-settled/u,
  );
  assert.equal(clicked, false);
  assert.equal(timing.steps.at(-1).step, "turn-action-blocked");
  assert.equal(timing.steps.at(-1).state, "completed");
});

test("every acceptance-matrix pair preserves capability admission at reachable widths", () => {
  const matrix = readFileSync(
    new URL(
      "../docs/work-items/CODEX-PARITY/acceptance-matrix.md",
      import.meta.url,
    ),
    "utf8",
  );
  const rows = matrix.split(/\r?\n/u).filter((line) => line.startsWith("| `"));
  assert.ok(rows.length > 50);
  const ids = new Set(nativeCases().map(({ id }) => id));
  for (const row of rows) {
    const columns = row.split("|");
    const state = columns[1].trim().slice(1, -1);
    for (const [index, harness] of ["codex", "claude", "pi"].entries()) {
      const absent = columns[index + 2].trim().startsWith("truthful-absent");
      const gap = columns[index + 2].trim().startsWith("gap:");
      assert.equal(
        gap,
        nativeCaptureProductGaps[harness].includes(state),
        `${harness}/${state}`,
      );
      for (const width of [1888, 720, 480]) {
        const args = [
          "--harness",
          harness,
          "--state",
          state,
          "--width",
          String(width),
        ];
        const responsiveWidth = {
          "responsive-wide": 1888,
          "responsive-720": 720,
          "responsive-480": 480,
        }[state];
        if (responsiveWidth && width !== responsiveWidth) {
          assert.throws(() => parseArgs(args), /responsive-width-mismatch/u);
          continue;
        }
        if (gap) {
          assert.throws(() => parseArgs(args), /product-gap-state/u);
        } else if (absent) {
          assert.equal(ids.has(`${harness}-${state}`), false);
          assert.throws(
            () => parseArgs(args),
            truthfulAbsentStates[harness].includes(state)
              ? /truthful-absent-state/u
              : /unsupported-state/u,
            `${harness}/${state}/${width}`,
          );
        } else {
          const options = parseArgs(args);
          assert.equal(options.caseId, `${harness}-${state}`);
          assert.ok(ids.has(options.caseId), options.caseId);
          assert.equal(options.height, width === 1888 ? 1096 : 1000);
        }
      }
    }
  }
});

test("blocked runs write their step timestamps to probe JSON without launching the app", async () => {
  const { capture } = await import("./capture-native-chat.mjs");
  const original = Object.getOwnPropertyDescriptor(globalThis, "WebSocket");
  let directory;
  try {
    Object.defineProperty(globalThis, "WebSocket", {
      value: undefined,
      configurable: true,
    });
    const options = parseArgs([
      "--harness",
      "claude",
      "--state",
      "turn-stopped",
      "--width",
      "1888",
    ]);
    const result = await capture(options);
    const root = path.resolve("work/native-captures");
    directory = path.resolve(root, result.run);
    assert.equal(path.relative(root, directory), result.run);
    const probe = JSON.parse(
      readFileSync(
        path.join(directory, "turn-stopped-claude-1888.probe.json"),
        "utf8",
      ),
    );
    assert.equal(probe.verdict, "blocked");
    assert.equal(probe.timing.steps[0].step, "capture-start");
    assert.ok(Number.isSafeInteger(probe.timing.steps[0].atMs));
    assert.ok(probe.timing.elapsedMs >= 0);
  } finally {
    if (original) Object.defineProperty(globalThis, "WebSocket", original);
    else delete globalThis.WebSocket;
    if (directory) rmSync(directory, { recursive: true });
  }
});

test("Cancel rechecks new-turn readiness at the click, refusing settlement and stale text", async () => {
  let clicked = 0;
  const control = {
    disabled: false,
    scrollIntoView() {},
    click() {
      clicked++;
    },
  };
  const expression = (observation) =>
    dom.liveActionExpression(
      JSON.stringify(observation),
      'button[aria-label="Cancel turn"]',
      "new-turn",
      "codex-app-server",
    );
  const document = { querySelectorAll: () => [control] };
  for (const changed of [
    { assistantRendered: false },
    { controls: { stop: false } },
  ])
    assert.equal(
      await vm.runInNewContext(expression({ ...ready, ...changed }), {
        document,
      }),
      false,
    );
  for (const state of dom.SETTLED_TURN_STATES)
    assert.equal(
      await vm.runInNewContext(expression({ ...ready, state }), { document }),
      "turn-action-settled",
    );
  assert.equal(
    await vm.runInNewContext(expression({ ...ready, turnId: "other-turn" }), {
      document,
    }),
    "turn-identity-drift",
  );
  assert.equal(clicked, 0);
  assert.equal(await vm.runInNewContext(expression(ready), { document }), true);
  assert.equal(clicked, 1);
});
