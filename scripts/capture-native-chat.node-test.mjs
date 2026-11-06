import assert from "node:assert/strict";
import test from "node:test";
import { CdpError } from "./source-functional-native-ide-cdp.mjs";
import { boundedEvaluate } from "./capture-native-chat-shared.mjs";
import path from "node:path";
import {
  mkdtempSync,
  mkdirSync,
  symlinkSync,
  existsSync,
  readFileSync,
  rmSync,
} from "node:fs";
import { ensurePlainDirectory } from "./capture-native-chat-process.mjs";
import {
  captureTiming,
  driveCaptureChat,
  cleanupCapture,
} from "./capture-native-chat-drive.mjs";
import {
  archivePreviousChatExpression,
  SINGLE_CAPTURE_PANE,
} from "./capture-native-chat-pane.mjs";
import {
  parseArgs,
  evidencePaths,
  validateProbe,
  selectOwnedTarget,
  discoverOwnedTarget,
  validateDimensions,
  validatePng,
  waitForState,
  withOwnedCapture,
  readBoundedResponse,
} from "./capture-native-chat-contract.mjs";

import {
  namedState,
  visibleRectangle,
  hiddenByClosedDetails,
  openExpression,
  isDisclosure,
  scopeToTurn,
  observationFor,
  actionsFor,
  HARNESS,
  IDLE_CAPTURE_STATES,
  clickExpression,
  liveActionExpression,
} from "./capture-native-chat-dom.mjs";
import { prepareCaptureChat } from "./capture-native-chat.mjs";

function captureChatBoundary(width, harness, created = {}, initial = {}) {
  const observation = {
    chatId: "welcome-chat",
    lazyFallbackVisible: false,
    projectId: "project",
    projectionLoaded: true,
    turnCount: 1,
    state: "completed",
    selected: "simulator",
    visible: {},
    controls: {},
    itemKinds: [],
    toolStates: [],
    ...initial,
  };
  const clicks = [];
  const archived = [];
  let paneCount = 1;
  // Agent navigation has no drawer at any capture width.
  const selectors = [
    'button[aria-label="New chat"]',
    'button[aria-label="Harness"]',
    '[role="option"][data-option-id="' + HARNESS[harness].id + '"]',
  ];
  const cdp = {
    async evaluate(expression) {
      if (expression === archivePreviousChatExpression("welcome-chat")) {
        archived.push("welcome-chat");
        paneCount = 1;
        return true;
      }
      if (expression === SINGLE_CAPTURE_PANE) return paneCount === 1;
      if (expression === observationFor(HARNESS[harness].binding))
        return { ...observation };
      const selector = selectors.find(
        (selector) => expression === clickExpression(selector),
      );
      assert.ok(selector, expression);
      clicks.push(selector);
      if (selector === selectors[0]) {
        paneCount = width === 1888 ? 2 : 1;
        Object.assign(
          observation,
          {
            chatId: "capture-chat",
            turnCount: 0,
            state: "none",
          },
          created,
        );
      }
      if (selector === selectors[2]) observation.selected = HARNESS[harness].id;
      return true;
    },
  };
  const expectedClicks = selectors;
  return { cdp, clicks, archived, observation, expectedClicks };
}

for (const width of [1888, 720, 480])
  test(`fresh capture archives only the disposable welcome chat at ${width}`, async () => {
    const boundary = captureChatBoundary(width, "claude");
    const captureChat = await prepareCaptureChat(
      boundary.cdp,
      "claude",
      Date.now() + 1000,
    );
    assert.deepEqual(captureChat, {
      chatId: "capture-chat",
      projectId: "project",
    });
    assert.deepEqual(boundary.archived, ["welcome-chat"]);
    assert.equal(boundary.observation.chatId, "capture-chat");
  });

for (const harness of ["codex", "claude", "pi"])
  for (const state of IDLE_CAPTURE_STATES)
    test(`${harness}/${state} capture predicates reject chat or pane ownership drift`, () => {
      const captureChat = { chatId: "capture-chat", projectId: "project" };
      const observation = {
        ...captureChat,
        lazyFallbackVisible: false,
        projectionLoaded: true,
        singlePane: true,
        selected: HARNESS[harness].id,
        bindingId: HARNESS[harness].binding,
        state: "none",
        turnCount: 0,
        controls: { stop: false, send: true },
        visible: { harnessMenu: true, addMenu: true },
      };
      const accepted = (sample) =>
        Object.values(
          namedState({ harness, state }, sample, captureChat),
        ).every(Boolean);
      assert.equal(accepted(observation), true);
      for (const drift of [
        { chatId: "another-empty-chat" },
        { projectId: "another-project" },
        { projectionLoaded: false },
        { singlePane: false },
        { singlePane: undefined },
      ])
        assert.equal(accepted({ ...observation, ...drift }), false);
    });

for (const [reason, unhydrated] of Object.entries({
  chat: { chatId: null },
  project: { projectId: null },
  projection: { projectionLoaded: false },
}))
  test(`capture waits for the previous ${reason} before creating a new chat`, async () => {
    const boundary = captureChatBoundary(1888, "pi");
    let observations = 0;
    const cdp = {
      async evaluate(expression) {
        if (
          expression === observationFor(HARNESS.pi.binding) &&
          boundary.clicks.length === 0
        ) {
          observations += 1;
          return {
            ...boundary.observation,
            ...(observations === 1 ? unhydrated : {}),
          };
        }
        assert.equal(observations, 2);
        return boundary.cdp.evaluate(expression);
      },
    };
    await prepareCaptureChat(cdp, "pi", Date.now() + 1000);
    assert.deepEqual(boundary.clicks, boundary.expectedClicks);
    assert.equal(boundary.observation.chatId, "capture-chat");
    assert.equal(boundary.observation.turnCount, 0);
  });

const args = [
  "--harness",
  "claude",
  "--state",
  "turn-completed",
  "--width",
  "720",
];
test("480 capture requests match the native minimum without lowering its height", () => {
  const config = JSON.parse(
    readFileSync(
      new URL("../src-tauri/tauri.conf.json", import.meta.url),
      "utf8",
    ),
  );
  assert.equal(config.app.windows[0].minWidth, 480);
  assert.equal(config.app.windows[0].minHeight, 560);
  for (const harness of ["codex", "claude", "pi"])
    for (const state of ["turn-completed", "responsive-480"])
      assert.deepEqual(
        parseArgs(["--harness", harness, "--state", state, "--width", "480"]),
        {
          harness,
          state,
          width: 480,
          height: 1000,
          caseId: `${harness}-${state}`,
        },
      );
});
test("native admission rejects product gaps instead of waiting for inert controls", () => {
  for (const [harness, state] of [
    ["codex", "model-menu"],
    ["codex", "file-edit-live"],
    ["codex", "file-hunk"],
    ["codex", "file-diff-tab"],
    ["codex", "attachment-chip"],
    ["claude", "attachment-chip"],
  ])
    assert.throws(
      () =>
        parseArgs(["--harness", harness, "--state", state, "--width", "720"]),
      /product-gap-state/u,
    );
  assert.equal(
    parseArgs([
      "--harness",
      "claude",
      "--state",
      "model-menu",
      "--width",
      "720",
    ]).state,
    "model-menu",
  );
});
for (const width of [1888, 720, 480])
  for (const harness of ["codex", "claude", "pi"]) {
    test(
      harness +
        " at " +
        width +
        " still prepares a fresh chat after empty project startup",
      async () => {
        const boundary = captureChatBoundary(
          width,
          harness,
          {},
          { turnCount: 0, state: "none" },
        );
        await prepareCaptureChat(boundary.cdp, harness, Date.now() + 1000);
        assert.equal(boundary.observation.turnCount, 0);
        assert.equal(boundary.observation.state, "none");
        assert.equal(boundary.observation.selected, HARNESS[harness].id);
        assert.deepEqual(boundary.clicks, boundary.expectedClicks);
      },
    );
    test(`${harness} at ${width} captures a hydrated empty new chat in the same project`, async () => {
      const boundary = captureChatBoundary(width, harness);
      await prepareCaptureChat(boundary.cdp, harness, Date.now() + 1000);
      assert.equal(
        namedState({ harness, state: "new-chat" }, boundary.observation)
          .namedState,
        true,
      );
      assert.deepEqual(boundary.clicks, boundary.expectedClicks);
    });
    for (const [reason, created] of Object.entries({
      unchanged: { chatId: "welcome-chat" },
      foreign: { projectId: "foreign-project" },
      unloaded: { projectionLoaded: false },
      welcome: { turnCount: 1, state: "completed" },
    }))
      test(`${harness} at ${width} rejects ${reason} chat before harness selection`, async () => {
        const boundary = captureChatBoundary(width, harness, created);
        await assert.rejects(
          prepareCaptureChat(boundary.cdp, harness, Date.now() + 25),
          /state-timeout/u,
        );
        assert.equal(boundary.observation.selected, "simulator");
      });
  }
test("ordinary Claude captures never change access preferences", async () => {
  const boundary = captureChatBoundary(1888, "claude");
  await driveCaptureChat(
    boundary.cdp,
    parseArgs([
      "--harness",
      "claude",
      "--state",
      "harness-menu",
      "--width",
      "1888",
    ]),
    Date.now() + 1000,
    () => {},
  );
  assert.ok(
    boundary.clicks.every(
      (selector) =>
        !selector.includes("Access options") &&
        !selector.includes("menuitemradio"),
    ),
  );
});
test("approval capture rejects missing advertised Ask before changing access or sending", async () => {
  const boundary = captureChatBoundary(1888, "claude", { accessModes: [] });
  await assert.rejects(
    driveCaptureChat(
      boundary.cdp,
      parseArgs([
        "--harness",
        "claude",
        "--state",
        "approval-approved",
        "--width",
        "1888",
      ]),
      Date.now() + 1000,
      () => {},
    ),
    /approval-access-unavailable/u,
  );
  assert.ok(
    boundary.clicks.every(
      (selector) =>
        !selector.includes("Access options") &&
        !selector.includes("Send message"),
    ),
  );
});

for (const state of ["approval-approved", "approval-declined"])
  test(`compact Claude ${state} capture leaves fifteen timing slots after cleanup`, async () => {
    const timing = captureTiming();
    const boundary = captureChatBoundary(720, "claude");
    const deadline = Date.now() + 1000;
    const observationExpression = observationFor(HARNESS.claude.binding);
    boundary.observation.accessModes = ["ask"];
    let warmupObservations = 0;
    let selectedAsk = false;
    const observation = {
      turnId: "none",
      turnCount: 0,
      state: "none",
      bindingId: HARNESS.claude.binding,
      controls: { stop: false, send: true, modelEnabled: true },
      assistantRendered: true,
    };
    const cdp = {
      async evaluate(expression) {
        if (boundary.observation.selected !== HARNESS.claude.id)
          return boundary.cdp.evaluate(expression);
        if (expression === observationExpression) {
          if (observation.turnCount === 1 && ++warmupObservations >= 2) {
            observation.state = "completed";
            observation.controls = {
              stop: false,
              send: true,
              modelEnabled: true,
            };
          }
          return { ...boundary.observation, ...observation };
        }
        if (
          expression === clickExpression('button[aria-label="Send message"]')
        ) {
          assert.equal(selectedAsk, true);
          assert.equal(observation.controls.send, true);
          assert.equal(observation.controls.stop, false);
          observation.turnCount++;
          observation.turnId = "turn-" + observation.turnCount;
          const warmup = observation.turnCount === 1;
          observation.state = warmup ? "running" : "waitingApproval";
          observation.controls = {
            stop: true,
            send: false,
            modelEnabled: false,
          };
        }
        if (
          expression === clickExpression('button[aria-label="Access options"]')
        ) {
          assert.equal(observation.turnCount, 0);
          assert.equal(observation.state, "none");
          assert.equal(observation.controls.modelEnabled, true);
        }
        if (
          expression ===
          clickExpression(
            '[role="menu"][aria-label="Access mode"] [role="menuitemradio"][data-option-id="ask"]',
          )
        ) {
          assert.equal(observation.turnCount, 0);
          selectedAsk = true;
        }
        if (
          expression ===
          liveActionExpression(
            observationExpression,
            scopeToTurn(actionsFor(state, "claude")[0], "turn-2"),
            "turn-2",
            HARNESS.claude.binding,
          )
        ) {
          assert.equal(observation.state, "waitingApproval");
          observation.state = "completed";
          observation.controls = {
            stop: false,
            send: true,
            modelEnabled: true,
          };
        }
        return true;
      },
    };
    for (const step of [
      "capture-start",
      "source-clean",
      "served-asset-ready",
      "app-launched",
      "cdp-target-ready",
      "window-ready",
    ])
      timing.record(step);
    assert.deepEqual(
      await driveCaptureChat(
        cdp,
        parseArgs(["--harness", "claude", "--state", state, "--width", "720"]),
        deadline,
        timing.record,
      ),
      { chatId: "capture-chat", projectId: "project" },
    );
    assert.equal(observation.turnCount, 2);
    assert.equal(observation.state, "completed");
    for (const step of [
      "layout-stable",
      "screenshot-start",
      "screenshot-captured",
      "capture-verified",
    ])
      timing.record(step, observation);
    assert.equal(
      await cleanupCapture(
        timing.record,
        ["cdp-close", "app-stop", "roots-cleanup"].map((step) => ({
          step,
          action: async () => {},
        })),
      ),
      undefined,
    );
    timing.record("source-reverified");
    timing.record("served-asset-reverified");
    assert.equal(
      await cleanupCapture(timing.record, [
        { step: "vite-stop", action: async () => {} },
      ]),
      undefined,
    );
    assert.equal(timing.steps.length, 49);
    const value = probe();
    value.timing.steps = timing.steps;
    assert.doesNotThrow(() => validateProbe(value));
    for (let remaining = 0; remaining < 15; remaining++)
      timing.record("reserved-cleanup");
    assert.throws(() => timing.record("extra"), /timing-bound/u);
  });
test("capture evaluations keep the overall deadline and a fresh five-second per-call bound", async (context) => {
  let now = 1000;
  context.mock.method(Date, "now", () => now);
  const calls = [];
  const cdp = {
    evaluate(expression, deadline) {
      calls.push([expression, deadline]);
      return Promise.resolve("observed");
    },
  };
  const evaluate = boundedEvaluate(cdp, 9000);
  assert.equal(await evaluate("first"), "observed");
  now = 2000;
  await evaluate("second");
  now = 8500;
  await evaluate("last");
  assert.deepEqual(calls, [
    ["first", 6000],
    ["second", 7000],
    ["last", 9000],
  ]);
});

test("capture arguments are closed, finite and reject duplicates", () => {
  assert.deepEqual(parseArgs(args), {
    harness: "claude",
    state: "turn-completed",
    width: 720,
    height: 1000,
    caseId: "claude-turn-completed",
  });
  for (const bad of [
    [],
    [...args, "--width", "720"],
    [...args, "--port", "1420"],
    args.map((value) => (value === "720" ? "481" : value)),
    args.map((value) => (value === "turn-completed" ? "../private" : value)),
  ])
    assert.throws(() => parseArgs(bad));
});
test("evidence paths cannot escape the selected directory", () => {
  const root = path.resolve("synthetic-evidence");
  assert.equal(
    evidencePaths(root, parseArgs(args)).png,
    path.join(root, "turn-completed-claude-720.png"),
  );
  for (const state of [
    "../private",
    "..\\private",
    "/private",
    "turn-completed:secret",
  ])
    assert.throws(() => evidencePaths(root, { ...parseArgs(args), state }));
});
test("capture explicitly rejects truthful-absent pairs", () => {
  for (const [harness, state] of [
    ["codex", "compaction-started"],
    ["codex", "compaction-failed"],
    ["codex", "compaction-cancelled"],
    ["codex", "approval-pending"],
    ["codex", "approval-approved"],
    ["codex", "approval-declined"],
    ["claude", "context-meter"],
    ["claude", "child-opened"],
  ])
    assert.throws(
      () =>
        parseArgs(["--harness", harness, "--state", state, "--width", "720"]),
      /truthful-absent-state/u,
    );
});
const probe = () => ({
  schemaVersion: 1,
  caseId: "claude-turn-completed",
  bindingId: "claude-agent-acp",
  turnId: "turn-synthetic",
  hashes: {
    build: "a".repeat(64),
    fixture: "b".repeat(64),
    source: "c".repeat(40),
    servedAsset: "d".repeat(64),
  },
  protocol: "standard-acp-1",
  webviewVersion: "130.0.1.0",
  dpr: 1,
  dimensions: {
    outer: { width: 496, height: 1039 },
    client: { width: 480, height: 1000 },
    css: { width: 480, height: 1000 },
  },
  predicates: { namedState: true, binding: true },
  assistantRendered: true,
  lazyFallbackVisible: false,
  elements: [],
  controls: { model: true },
  overflow: { horizontal: false },
  timing: { elapsedMs: 100, steps: [] },
  verdict: "captured",
});
test("Pi capture and probe protocols agree with the binding version constant", () => {
  const manifest = readFileSync(
    new URL("../src-tauri/src/acp/bindings/pi/manifest.rs", import.meta.url),
    "utf8",
  );
  const version = manifest.match(/pub const PI_VERSION: &str = "([^"]+)";/u);
  assert.ok(version);
  const protocol = `pi-rpc-${version[1]}`;
  assert.equal(HARNESS.pi.protocol, protocol);
  const value = {
    ...probe(),
    caseId: "pi-turn-completed",
    bindingId: HARNESS.pi.binding,
    protocol,
  };
  assert.equal(validateProbe(value), value);
});
test("probe JSON records boolean rendered and lazy-fallback witnesses and fails closed", () => {
  const value = JSON.parse(JSON.stringify(probe()));
  assert.equal(validateProbe(value).assistantRendered, true);
  assert.equal(value.lazyFallbackVisible, false);
  for (const field of ["assistantRendered", "lazyFallbackVisible"])
    for (const invalid of [undefined, null, "false"])
      assert.throws(
        () => validateProbe({ ...value, [field]: invalid }),
        /probe-rendered/u,
      );
  assert.throws(
    () => validateProbe({ ...value, lazyFallbackVisible: true }),
    /state-predicate/u,
  );
});
test("probe exact limits pass; plus-one, privacy and unknown fields fail", () => {
  const exact = probe();
  exact.elements = Array.from({ length: 64 }, () => ({
    kind: "tool",
    text: "x".repeat(512),
    x: 0,
    y: 0,
    width: 10,
    height: 20,
  }));
  assert.equal(validateProbe(exact), exact);
  for (const mutate of [
    (value) => value.elements.push(value.elements[0]),
    (value) => (value.elements[0].text += "x"),
    (value) => (value.elements[0].text = "sk-ant-private"),
    (value) => (value.dpr = 1.25),
    (value) => (value.bindingId = "codex-app-server"),
    (value) => (value.protocol = "codex-app-server-0.146.0"),
    (value) => (value.dump = "private"),
    (value) => (value.hashes.servedAsset = "d".repeat(63)),
    (value) => (value.caseId = "claude-context-meter"),
    (value) => (value.elements[0].x = Infinity),
    (value) => (value.elements[0].text = "C:\\Users\\someone\\secret"),
  ]) {
    const value = structuredClone(exact);
    mutate(value);
    assert.throws(() => validateProbe(value));
  }
});
test("DPR, clamp and PNG viewport mismatches fail closed", () => {
  const value = probe();
  assert.doesNotThrow(() => validateDimensions(value.dimensions, 480, 1000, 1));
  assert.throws(() => validateDimensions(value.dimensions, 720, 1000, 1));
  assert.throws(() => validateDimensions(value.dimensions, 480, 1000, 2));
  const png = Buffer.alloc(24);
  Buffer.from("89504e470d0a1a0a", "hex").copy(png);
  png.write("IHDR", 12);
  png.writeUInt32BE(480, 16);
  png.writeUInt32BE(1000, 20);
  assert.doesNotThrow(() => validatePng(png, value.dimensions.css));
  assert.throws(() => validatePng(png, { width: 720, height: 1000 }));
});
test("probe timestamps have a finite closed schema and a 64-step limit", () => {
  const timing = captureTiming();
  for (let index = 0; index < 64; index++)
    timing.record("turn-observed", {
      turnId: "new-turn",
      state: "running",
      controls: { stop: true },
      assistantRendered: true,
    });
  const value = probe();
  value.timing.steps = timing.steps;
  assert.equal(validateProbe(value), value);
  assert.throws(() => timing.record("extra"), /timing-bound/u);
  for (const mutate of [
    (value) => value.timing.steps.push(value.timing.steps[0]),
    (value) => (value.timing.steps[0].atMs = Infinity),
    (value) => (value.timing.steps[0].turnId = "private-sentinel"),
    (value) => (value.timing.steps[0].extra = true),
    (value) => (value.timing.steps[0].cancelPresent = "yes"),
  ]) {
    const changed = structuredClone(value);
    mutate(changed);
    assert.throws(() => validateProbe(changed));
  }
});
test("a CDP deadline after the state deadline becomes a state timeout with its cause", async () => {
  const failure = new CdpError("deadline");
  const deadline = Date.now() + 50;
  await assert.rejects(
    waitForState(async () => {
      await new Promise((resolve) => setTimeout(resolve, 75));
      throw failure;
    }, deadline),
    (error) => {
      assert.equal(error.message, "state-timeout");
      assert.equal(error.cause, failure);
      return true;
    },
  );
});

test("a CDP deadline before the state deadline propagates unchanged", async () => {
  const failure = new CdpError("deadline");
  await assert.rejects(
    waitForState(async () => {
      await new Promise((resolve) => setTimeout(resolve, 10));
      throw failure;
    }, Date.now() + 60_000),
    (error) => error === failure,
  );
});

test("a predicate error after the state deadline propagates unchanged", async () => {
  const failure = new Error("turn-identity-drift");
  const deadline = Date.now() + 50;
  await assert.rejects(
    waitForState(async () => {
      await new Promise((resolve) => setTimeout(resolve, 75));
      throw failure;
    }, deadline),
    (error) => error === failure,
  );
});

test("stalled predicates are bounded and teardown runs on failure", async () => {
  await assert.rejects(
    waitForState(async () => false, Date.now() + 5),
    /state-timeout/u,
  );
  const actions = [];
  await assert.rejects(
    withOwnedCapture(
      async () => {
        throw Error("blocked");
      },
      async () => actions.push("cdp"),
      async () => actions.push("owned-process"),
    ),
    /blocked/u,
  );
  assert.deepEqual(actions, ["cdp", "owned-process"]);
});

test("named states do not mistake settled compaction or allowed approvals", () => {
  const observation = {
    lazyFallbackVisible: false,
    bindingId: "codex-app-server",
    selected: "codex",
    state: "running",
    visible: { compaction: true, permission: true },
    itemKinds: [],
    controls: {},
    compactionStates: ["completed"],
    permissionStates: ["resolvedAllowed"],
  };
  const options = { harness: "codex", state: "compaction-started" };
  const check = (state, changes, harness = "codex") =>
    namedState({ harness, state }, { ...observation, ...changes }).namedState;
  assert.equal(namedState(options, observation).namedState, false);
  assert.equal(check("compaction-cancelled", { state: "cancelled" }), false);
  assert.equal(
    check("approval-declined", { state: "completed" }, "claude"),
    false,
  );
  assert.equal(check("compaction-completed", { state: "completed" }), true);
  const wrongBinding = namedState(options, {
    ...observation,
    bindingId: "foreign",
    compactionStates: ["started"],
  });
  assert.equal(wrongBinding.binding, false);
  assert.equal(
    check(options.state, { visible: {}, compactionStates: ["started"] }),
    false,
  );
});
test("app exit remains the primary capture error when teardown also fails", async () => {
  const exit = Object.assign(new Error("owned-app-exited"), {
    appExit: { code: 101, signal: null, reason: "harness-transport-contract" },
  });
  await assert.rejects(
    withOwnedCapture(
      async () => {
        throw exit;
      },
      async () => {
        throw new Error("native-tool-blocked");
      },
      async () => {},
    ),
    (error) => error === exit,
  );
});

test("only rendered rectangles inside the viewport can prove visible state", () => {
  const viewport = { width: 480, height: 1000 };
  for (const [left, top, width, expected] of [
    [0, 0, 20, true],
    [480, 0, 20, false],
    [0, -20, 20, false],
    [0, 0, 0, false],
    [NaN, 0, 20, false],
  ])
    assert.equal(
      visibleRectangle({ x: left, y: top, width, height: 20 }, viewport),
      expected,
    );
});
test("CDP discovery body exact bound passes; plus-one cancels the stream", async () => {
  let cancelled = false;
  const response = (length) => ({
    body: new ReadableStream({
      start(controller) {
        controller.enqueue(new Uint8Array(length).fill(32));
        if (length <= 65536) controller.close();
      },
      cancel() {
        cancelled = true;
      },
    }),
  });
  assert.equal((await readBoundedResponse(response(65536))).length, 65536);
  await assert.rejects(
    readBoundedResponse(response(65537)),
    /cdp-discovery-bound/u,
  );
  assert.equal(cancelled, true);
});

test("directory creation rejects linked ancestors before creating descendants", (context) => {
  const root = mkdtempSync(path.join(process.cwd(), ".nc1-path-test-"));
  try {
    const target = path.join(root, "target"),
      link = path.join(root, "link");
    mkdirSync(target);
    try {
      symlinkSync(
        target,
        link,
        process.platform === "win32" ? "junction" : "dir",
      );
    } catch (error) {
      if (["EPERM", "EACCES", "ENOSYS"].includes(error.code)) {
        context.skip("links unavailable");
        return;
      }
      throw error;
    }
    assert.throws(
      () => ensurePlainDirectory(path.join(link, "uncreated")),
      /path-reparse/u,
    );
    assert.equal(existsSync(path.join(target, "uncreated")), false);
    assert.equal(
      ensurePlainDirectory(path.join(root, "plain", "nested")),
      path.join(root, "plain", "nested"),
    );
  } finally {
    rmSync(root, { recursive: true });
  }
});

test("finished capture cells never open Worked or tool disclosures", () => {
  for (const state of [
    "tool-finished",
    "tools-grouped",
    "git-finished",
    "tool-long-label",
  ])
    assert.deepEqual(actionsFor(state), []);
  assert.deepEqual(actionsFor("approval-approved"), [
    ".acp-permission-prompt__split > button:first-child",
  ]);
  assert.deepEqual(actionsFor("approval-declined"), [
    ".acp-permission-prompt__actions > button:not(.acp-permission-prompt__stop)",
  ]);
});

test("plan entry requires an offered plan mode, not a permission control", () => {
  const observation = {
    state: "completed",
    visible: { addMenu: true, access: true },
    itemKinds: [],
    controls: {},
  };
  assert.equal(
    namedState({ harness: "claude", state: "plan-mode-entry" }, observation)
      .namedState,
    false,
  );
  assert.deepEqual(actionsFor("approval-approved"), [
    ".acp-permission-prompt__split > button:first-child",
  ]);
});
test("disclosure actions target only the latest turn, not the project welcome turn", () => {
  for (const state of ["tool-finished", "tools-grouped", "tool-expanded"])
    for (const selector of actionsFor(state))
      assert.match(selector, /^.acp-turn:last-of-type /u);
});
function node(tag, { open = false, parent = null } = {}) {
  const element = { tag, open, parentElement: parent, children: [] };
  if (parent) parent.children.push(element);
  element.contains = (other) => {
    for (let current = other; current; current = current.parentElement)
      if (current === element) return true;
    return false;
  };
  element.closest = (selector) => {
    assert.equal(selector, "details:not([open])");
    for (let current = element; current; current = current.parentElement)
      if (current.tag === "details" && !current.open) return current;
    return null;
  };
  element.querySelector = (selector) => {
    assert.equal(selector, ":scope > summary");
    return element.children.find((child) => child.tag === "summary") ?? null;
  };
  return element;
}
test("a closed disclosure row is visible; its body and closed ancestors hide content", () => {
  const work = node("details", { open: true });
  const rollup = node("details", { parent: work });
  const summary = node("summary", { parent: rollup });
  const label = node("span", { parent: summary });
  const body = node("div", { parent: rollup });
  assert.equal(hiddenByClosedDetails(rollup), false);
  assert.equal(hiddenByClosedDetails(label), false);
  assert.equal(hiddenByClosedDetails(body), true);
  work.open = false;
  node("summary", { parent: work });
  assert.equal(hiddenByClosedDetails(rollup), true);
  assert.equal(hiddenByClosedDetails(label), true);
});
test("disclosure opening is idempotent and refuses ambiguous or non-details targets", () => {
  const run = (elements) =>
    new Function("document", "return " + openExpression("x > summary"))({
      querySelectorAll: () => elements,
    });
  let clicks = 0;
  const details = { tagName: "DETAILS", open: false };
  const summary = {
    parentElement: details,
    scrollIntoView() {},
    click() {
      clicks += 1;
      details.open = !details.open;
    },
  };
  assert.equal(run([summary]), true);
  assert.equal(run([summary]), true);
  assert.equal(clicks, 1);
  assert.equal(run([summary, { ...summary }]), false);
  assert.equal(run([{ ...summary, parentElement: { tagName: "DIV" } }]), false);
  assert.equal(run([]), false);
  assert.equal(
    isDisclosure(".acp-turn:last-of-type .chat-transcript-work > summary"),
    true,
  );
  assert.equal(isDisclosure('button[aria-label="Cancel turn"]'), false);
});
test("live predicates accept waiting turn states but not queued or settled ones", () => {
  const observation = (state) => ({
    lazyFallbackVisible: false,
    state,
    visible: { pendingApproval: true, thought: true },
    itemKinds: [],
    toolStates: [],
    controls: {},
  });
  const pending = (state) =>
    namedState(
      { harness: "claude", state: "approval-pending" },
      observation(state),
    ).namedState;
  assert.equal(pending("waitingApproval"), true);
  assert.equal(pending("running"), true);
  for (const state of [
    "queued",
    "sending",
    "cancelling",
    "completed",
    "failed",
  ])
    assert.equal(pending(state), false);
});
test("latest-turn actions are pinned to the observed turn identity", () => {
  assert.equal(
    scopeToTurn(
      ".acp-turn:last-of-type .chat-transcript-work > summary",
      "turn-1",
    ),
    '.acp-turn[data-turn-id="turn-1"] .chat-transcript-work > summary',
  );
  assert.equal(
    scopeToTurn('button[aria-label="Cancel turn"]', "none"),
    'button[aria-label="Cancel turn"]',
  );
  for (const turnId of ['x"] *', "none]", ""])
    assert.throws(
      () => scopeToTurn(".acp-turn:last-of-type summary", turnId),
      /turn-identity/u,
    );
});
test("observation pins the expected binding and rejects injected identities", () => {
  assert.match(
    observationFor("claude-agent-acp"),
    /const expectedBinding="claude-agent-acp";/u,
  );
  for (const binding of ['x";alert(1);"', "", "A".repeat(65)])
    assert.throws(() => observationFor(binding), /binding-identity/u);
});
test("hydrated empty chats bind to the selected harness, not a retained welcome session", async () => {
  for (const harness of ["codex", "claude", "pi"]) {
    const selected = HARNESS[harness];
    const store = {
      activeTabId: "empty-chat",
      project: { id: "project" },
      tabs: [{ id: "empty-chat", kind: "chat" }],
      harnesses: [{ id: selected.id, bindingId: selected.binding }],
    };
    const page = {
      chatId: "empty-chat",
      projectId: "project",
      turns: [],
      sessions: [{ id: "welcome-session", bindingId: "simulator" }],
    };
    const expression = observationFor(selected.binding)
      .replace("await import('/src/app/store.ts')", "workbench")
      .replace(
        "await import('/src/app/agentChatOwner.ts')",
        "({pinnedAgentChatOwner:()=>workbench.useWorkbenchStore.getState().activeTabId})",
      )
      .replace("await import('/src/app/acpProjectionRegistry.ts')", "registry");
    const observe = new Function(
      "workbench",
      "registry",
      "document",
      "devicePixelRatio",
      "innerWidth",
      "innerHeight",
      "return " + expression,
    );
    const observation = await observe(
      { useWorkbenchStore: { getState: () => store } },
      { acpProjectionStoreFor: () => ({ getState: () => ({ page }) }) },
      {
        querySelector: () => ({ getAttribute: () => selected.id }),
        querySelectorAll: (selector) =>
          selector.endsWith('button[aria-label="Harness"]')
            ? [
                {
                  getAttribute: () => selected.id,
                  getClientRects: () => [],
                  getBoundingClientRect: () => ({
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                  }),
                },
              ]
            : [],
        documentElement: { scrollWidth: 720 },
      },
      1,
      720,
      1000,
    );
    assert.equal(observation.projectionLoaded, true);
    assert.equal(observation.bindingId, selected.binding);
    assert.equal(observation.turnCount, 0);
    for (const state of ["new-chat", "composer-idle", "harness-menu"]) {
      const predicates = namedState(
        { harness, state },
        {
          ...observation,
          controls: { stop: false, send: true },
          visible: { harnessMenu: true },
        },
      );
      assert.deepEqual(predicates, {
        binding: true,
        selected: true,
        namedState: true,
      });
    }
  }
});
const ownedTarget = {
  id: "owned",
  type: "page",
  title: "Sage",
  url: "http://127.0.0.1:1420/",
  webSocketDebuggerUrl: "ws://127.0.0.1:9223/devtools/page/owned",
};
test("target selection waits on absent or untitled pages and rejects ambiguity", () => {
  assert.equal(selectOwnedTarget([], 9223), null);
  assert.equal(
    selectOwnedTarget(
      [{ ...ownedTarget, url: "about:blank", title: "" }],
      9223,
    ),
    null,
  );
  assert.equal(
    selectOwnedTarget([{ ...ownedTarget, title: "127.0.0.1:1420/" }], 9223),
    null,
  );
  assert.equal(selectOwnedTarget([ownedTarget], 9223), ownedTarget);
  assert.throws(
    () => selectOwnedTarget([ownedTarget, ownedTarget], 9223),
    /cdp-target/u,
  );
  assert.throws(
    () =>
      selectOwnedTarget(
        [{ ...ownedTarget, webSocketDebuggerUrl: "ws://evil/" }],
        9223,
      ),
    /cdp-target/u,
  );
  assert.throws(() => selectOwnedTarget({}, 9223), /cdp-target/u);
  assert.throws(
    () => selectOwnedTarget(Array(33).fill({}), 9223),
    /cdp-target/u,
  );
});
test("target title boundaries admit Sage suffixes but reject prefix lookalikes and mixed ambiguity", () => {
  const suffixed = { ...ownedTarget, title: "Sage synthetic project" };
  assert.equal(selectOwnedTarget([suffixed], 9223), suffixed);
  assert.equal(
    selectOwnedTarget([{ ...ownedTarget, title: "Sages-other" }], 9223),
    null,
  );
  const second = {
    ...suffixed,
    id: "second",
    webSocketDebuggerUrl: "ws://127.0.0.1:9223/devtools/page/second",
  };
  assert.throws(
    () => selectOwnedTarget([ownedTarget, second], 9223),
    /cdp-target/u,
  );
});
test("discovery polls until the owned page appears and fails closed at its deadline", async () => {
  const listings = [
    undefined,
    [],
    [{ ...ownedTarget, title: "" }],
    [ownedTarget],
  ];
  let polls = 0;
  assert.equal(
    await discoverOwnedTarget(
      async () => listings[polls++],
      9223,
      Date.now() + 5000,
    ),
    ownedTarget,
  );
  assert.equal(polls, 4);
  await assert.rejects(
    discoverOwnedTarget(async () => [], 9223, Date.now() + 120),
    /^Error: cdp-target$/u,
  );
  await assert.rejects(
    discoverOwnedTarget(
      async () => [ownedTarget, ownedTarget],
      9223,
      Date.now() + 5000,
    ),
    /cdp-target/u,
  );
  await assert.rejects(
    discoverOwnedTarget(
      async () => {
        throw new Error("owned-app-exited");
      },
      9223,
      Date.now() + 5000,
    ),
    /owned-app-exited/u,
  );
});
