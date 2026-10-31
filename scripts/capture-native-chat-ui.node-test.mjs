// Browser conformance lane for the native chat capture tooling. It renders the
// visual fixture Chats in the real frontend (an owned Vite server and bundled
// Chromium, the engine WebView2 embeds) and drives them with the capture
// tool's own CDP client, observation probe, actions and parity steps, then
// checks the tool's own named-state predicate. A product change that strands
// a capture selector or flow fails here instead of in a native capture run.
//
// The fixtures hold settled Turns, so only states reachable from a settled
// Turn without a replay backend are driven: live Turns, approvals, diffs with
// saved refs, forks, subagents and seeded find text need the native replay.
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { createServer } from "node:net";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  discoverOwnedTarget,
  parseArgs,
  readBoundedResponse,
  waitForState,
} from "./capture-native-chat-contract.mjs";
import {
  ACCESS_OPTIONS,
  AGENT_MODE,
  COMPOSER_FILL,
  HARNESS,
  HARNESS_PICKER,
  NEW_TASK,
  SEND_MESSAGE,
  accessModeOption,
  clickExpression,
  clickableExpression,
  harnessOption,
  namedState,
  observationFor,
} from "./capture-native-chat-dom.mjs";
import { runCaptureActions } from "./capture-native-chat-drive.mjs";
import { boundedEvaluate } from "./capture-native-chat-shared.mjs";
import { settleDevServer, startDevServer } from "./lib/app-smoke-server.mjs";
import { connectCdp } from "./source-functional-native-ide-cdp.mjs";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const { chromium } = createRequire(import.meta.url)("playwright");
const OPEN_MS = 60_000;
const DRIVE_MS = 20_000;
const SETTLE_MS = 5_000;
const WIDTHS = [1888, 720, 480];

/**
 * [fixture scenario, harness, capture state]. Each state runs at every width
 * the capture CLI admits for it. Codex has no model picker without an account
 * (a catalogued product gap), so the model menu is driven as Claude.
 */
const CELLS = Object.freeze([
  ["agent-single", "codex", "turn-completed"],
  ["agent-single", "codex", "composer-idle"],
  ["agent-single", "codex", "responsive-wide"],
  ["agent-single", "codex", "responsive-720"],
  ["agent-single", "codex", "responsive-480"],
  ["agent-single", "codex", "harness-menu"],
  ["agent-single", "claude", "model-menu"],
  ["agent-single", "codex", "access-menu"],
  ["agent-single", "codex", "files-pill"],
  ["agent-command-rollup", "codex", "worked-expanded"],
  ["agent-command-rollup", "codex", "tool-expanded"],
  // Every finished answer has a "…"; a fresh capture Chat holds one Turn.
  ["agent-command-rollup", "codex", "fork-menu"],
  ["agent-session-mode-closed", "codex", "add-menu"],
  ["agent-composer-context-known", "codex", "context-meter"],
]);

/** Two distinct free loopback ports; both probes stay bound until both are read. */
async function reserveFreePorts() {
  const listen = () =>
    new Promise((done, fail) => {
      const probe = createServer();
      probe.once("error", fail);
      probe.listen(0, "127.0.0.1", () => done(probe));
    });
  const probes = [await listen(), await listen()];
  const ports = probes.map((probe) => probe.address().port);
  await Promise.all(
    probes.map((probe) => new Promise((done) => probe.close(done))),
  );
  return ports;
}

/** The capture options the CLI would admit, or null for a refused width. */
function admitted(harness, state, width) {
  try {
    return parseArgs([
      "--harness",
      harness,
      "--state",
      state,
      "--width",
      String(width),
    ]);
  } catch (error) {
    if (
      ["responsive-width-mismatch", "state-width-unsupported"].includes(
        error.message,
      )
    )
      return null;
    throw error;
  }
}

let lane = null;
test.before(async () => {
  const [vitePort, cdpPort] = await reserveFreePorts();
  const url = `http://127.0.0.1:${vitePort}/`;
  const server = await startDevServer(repositoryRoot, url);
  try {
    const browser = await chromium.launch({
      headless: true,
      args: [
        "--remote-debugging-address=127.0.0.1",
        `--remote-debugging-port=${cdpPort}`,
      ],
    });
    lane = { url, cdpPort, server, browser };
  } catch (error) {
    await settleDevServer(server);
    throw error;
  }
});
test.after(async () => {
  if (!lane) return;
  try {
    await lane.browser.close();
  } finally {
    assert.equal(await settleDevServer(lane.server), true);
  }
});

/**
 * Opens one owned page on the fixture Chat and attaches the capture CDP
 * client to it exactly as a native capture attaches to WebView2.
 */
async function openCapture(t, scenario, options) {
  const deadline = Date.now() + OPEN_MS;
  const context = await lane.browser.newContext({
    viewport: { width: options.width, height: options.height },
    colorScheme: "dark",
  });
  t.after(() => context.close());
  // Start in Editor so the capture's own Agent mode switch is exercised.
  await context.addInitScript(() =>
    localStorage.setItem(
      "sage.workbench.layout.v1",
      JSON.stringify({
        editorToolRailWidth: 264,
        agentChatSplitPercent: 50,
        windowMode: "editor",
      }),
    ),
  );
  const page = await context.newPage();
  await page.goto(lane.url);
  await page.locator("main.workbench").waitFor();
  const target = await discoverOwnedTarget(
    async () => {
      let response;
      try {
        response = await fetch(`http://127.0.0.1:${lane.cdpPort}/json`, {
          signal: AbortSignal.timeout(1_000),
        });
      } catch {
        return undefined;
      }
      if (!response.ok) return undefined;
      return JSON.parse(await readBoundedResponse(response));
    },
    lane.cdpPort,
    deadline,
    lane.url,
  );
  const cdp = await connectCdp({ target, deadline });
  t.after(() => cdp.close(Date.now() + 5_000));
  const evaluate = boundedEvaluate(cdp, deadline);
  assert.equal(
    await evaluate(
      `(async()=>{const fixtures=await import('/tests/ui/fixtures/visualScenarios.ts');await fixtures.installVisualScenario(${JSON.stringify(scenario)});return document.documentElement.dataset.visualScenario})()`,
    ),
    scenario,
  );
  await waitForState(() => evaluate(AGENT_MODE), deadline);
  const observe = () =>
    evaluate(observationFor(HARNESS[options.harness].binding));
  let observation;
  await waitForState(async () => {
    observation = await observe();
    return (
      observation.projectionLoaded === true &&
      observation.singlePane === true &&
      observation.lazyFallbackVisible === false
    );
  }, deadline);
  return {
    cdp,
    evaluate,
    observe,
    captureChat: {
      chatId: observation.chatId,
      projectId: observation.projectId,
    },
    turnId: observation.turnId,
  };
}

async function reached(capture, options) {
  let predicates;
  try {
    await waitForState(async () => {
      predicates = namedState(
        options,
        await capture.observe(),
        capture.captureChat,
      );
      return predicates.namedState;
    }, Date.now() + SETTLE_MS);
  } catch (error) {
    if (error.message !== "state-timeout") throw error;
  }
  return predicates;
}

function drive(capture, options, deadline = Date.now() + DRIVE_MS) {
  return runCaptureActions({
    cdp: capture.cdp,
    options,
    captureChat: capture.captureChat,
    submittedTurnId: capture.turnId,
    submit: async () => {
      throw new Error("conformance-submit-unavailable");
    },
    deadline,
    record: () => {},
  });
}

for (const [scenario, harness, state] of CELLS)
  for (const width of WIDTHS) {
    const options = admitted(harness, state, width);
    if (!options) continue;
    test(
      `${harness} ${state} at ${width} is driven and witnessed on the rendered ${scenario} Chat`,
      { timeout: OPEN_MS + DRIVE_MS + SETTLE_MS },
      async (t) => {
        const capture = await openCapture(t, scenario, options);
        await drive(capture, options);
        const predicates = await reached(capture, options);
        assert.equal(predicates.namedState, true, JSON.stringify(predicates));
        if ("chat" in predicates) {
          assert.equal(predicates.chat, true);
          assert.equal(predicates.singlePane, true);
        }
      },
    );
  }

/** Each prepare/submit control resolves to one enabled match, as a click needs. */
async function clickable(capture, selector) {
  return capture.evaluate(clickableExpression(selector));
}

for (const width of WIDTHS)
  test(
    `the prepare and submit controls resolve to one enabled match at ${width}`,
    { timeout: OPEN_MS + DRIVE_MS },
    async (t) => {
      const capture = await openCapture(
        t,
        "agent-single",
        admitted("codex", "composer-idle", width),
      );
      // New task is reachable without a navigation drawer at every width.
      for (const control of [NEW_TASK, HARNESS_PICKER, ACCESS_OPTIONS])
        assert.equal(await clickable(capture, control), true, control);
      const { selected } = await capture.observe();
      assert.equal(
        await capture.evaluate(clickExpression(HARNESS_PICKER)),
        true,
      );
      await waitForState(
        () => clickable(capture, harnessOption(selected)),
        Date.now() + SETTLE_MS,
      );
      await capture.evaluate(clickExpression(HARNESS_PICKER));
      assert.equal(
        await capture.evaluate(clickExpression(ACCESS_OPTIONS)),
        true,
      );
      for (const mode of ["readOnly", "workspaceWrite"])
        await waitForState(
          () => clickable(capture, accessModeOption(mode)),
          Date.now() + SETTLE_MS,
        );
      await capture.evaluate(clickExpression(ACCESS_OPTIONS));
      assert.equal(await capture.evaluate(COMPOSER_FILL), true);
      await waitForState(
        () => clickable(capture, SEND_MESSAGE),
        Date.now() + SETTLE_MS,
      );
    },
  );

const SEND = `document.querySelector('.agent-chat-pane--focused button[aria-label="Send message"]')`;
for (const [drift, mutation] of [
  [
    "missing",
    `(()=>{const send=${SEND};send?.remove();return Boolean(send)})()`,
  ],
  [
    "duplicated",
    `(()=>{const send=${SEND};send?.after(send.cloneNode(true));return Boolean(send)})()`,
  ],
])
  test(
    `a ${drift} Send control is not clickable, so submission fails closed`,
    { timeout: OPEN_MS },
    async (t) => {
      const capture = await openCapture(
        t,
        "agent-single",
        admitted("codex", "composer-idle", 1888),
      );
      assert.equal(await capture.evaluate(COMPOSER_FILL), true);
      await waitForState(
        () => clickable(capture, SEND_MESSAGE),
        Date.now() + SETTLE_MS,
      );
      assert.equal(await capture.evaluate(mutation), true);
      assert.equal(await clickable(capture, SEND_MESSAGE), false);
    },
  );

const PICKER = `document.querySelector('.agent-chat-pane--focused button[aria-label="Harness"]')`;
for (const [drift, mutation] of [
  [
    "renamed",
    `(()=>{const picker=${PICKER};picker?.setAttribute('aria-label','Agent');return Boolean(picker)})()`,
  ],
  [
    "duplicated",
    `(()=>{const picker=${PICKER};picker?.after(picker.cloneNode(true));return Boolean(picker)})()`,
  ],
])
  test(
    `a ${drift} capture control fails the lane instead of driving a guess`,
    { timeout: OPEN_MS + DRIVE_MS },
    async (t) => {
      const options = admitted("codex", "harness-menu", 1888);
      const capture = await openCapture(t, "agent-single", options);
      assert.equal(await capture.evaluate(mutation), true);
      await assert.rejects(
        drive(capture, options, Date.now() + 2_000),
        /state-timeout/u,
      );
      assert.equal((await reached(capture, options)).namedState, false);
    },
  );

test(
  "the composer still hides its context meter at 480, which is why the CLI refuses it there",
  { timeout: OPEN_MS },
  async (t) => {
    // parseArgs refuses context-meter at 480; open the Chat at that size.
    const options = admitted("codex", "turn-completed", 480);
    const capture = await openCapture(
      t,
      "agent-composer-context-known",
      options,
    );
    const observation = await capture.observe();
    assert.equal(observation.visible.meter, false);
    assert.equal(observation.controls.send, true);
    assert.equal(admitted("codex", "context-meter", 480), null);
  },
);
