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
  NEW_CHAT,
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
import { resolveUiPort } from "./lib/ui-test-port.mjs";
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
 * (a catalogued product gap), so the model menu is driven as Claude. A single
 * advertised model renders no picker, so the model menu needs the fixture
 * whose harness advertises many models once selected and ready.
 */
const CELLS = Object.freeze([
  ["agent-single", "codex", "turn-completed"],
  ["agent-single", "codex", "composer-idle"],
  ["agent-single", "codex", "responsive-wide"],
  ["agent-single", "codex", "responsive-720"],
  ["agent-single", "codex", "responsive-480"],
  ["agent-single", "codex", "harness-menu"],
  ["agent-picker-harness-model", "claude", "model-menu"],
  ["agent-single", "codex", "access-menu"],
  ["agent-single", "codex", "files-pill"],
  ["agent-command-rollup", "codex", "worked-expanded"],
  ["agent-command-rollup", "codex", "tool-expanded"],
  // Every finished answer has a "…"; a fresh capture Chat holds one Turn.
  ["agent-command-rollup", "codex", "fork-menu"],
  ["agent-session-mode-closed", "codex", "add-menu"],
  ["agent-composer-context-known", "codex", "context-meter"],
]);

/** A free loopback port for the Chromium CDP endpoint. */
async function reserveFreePort() {
  const probe = await new Promise((done, fail) => {
    const server = createServer();
    server.once("error", fail);
    server.listen(0, "127.0.0.1", () => done(server));
  });
  const { port } = probe.address();
  await new Promise((done) => probe.close(done));
  return port;
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
  // The UI port has one owner per run: SAGE_UI_PORT (default 1431), as for
  // Playwright, so concurrent UI lanes take distinct ports by configuration.
  const url = `http://127.0.0.1:${resolveUiPort()}/`;
  const server = await startDevServer(repositoryRoot, url);
  try {
    // Reserved once Vite holds the UI port, so the two cannot coincide.
    const cdpPort = await reserveFreePort();
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
      observation.lazyFallbackVisible === false &&
      // The composer mounts after the projection; slow runners show the gap.
      observation.controls.send === true &&
      observation.controls.harness === true &&
      typeof observation.selected === "string"
    );
  }, deadline);
  return {
    page,
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

/**
 * Fixture setup a scenario needs before its state is reachable. The picker
 * fixture starts on an unavailable harness: the capture's own harness controls
 * select the advertised one, then the fixture releases its readiness gate, as
 * a native harness reports ready, and its 128 models enable the Model picker.
 */
const PREPARE = Object.freeze({
  "agent-picker-harness-model": async (capture) => {
    await settled(() => capture.evaluate(clickExpression(HARNESS_PICKER)));
    await settled(() =>
      capture.evaluate(clickExpression(harnessOption("visual-acp"))),
    );
    await settled(
      async () => (await capture.observe()).selected === "visual-acp",
      "harness selection",
    );
    assert.equal(
      await capture.evaluate(
        "(()=>{const release=window.__releaseVisualPickerHarnessReady;if(!release)return false;release();return true})()",
      ),
      true,
    );
    await settled(
      async () => (await capture.observe()).controls.modelEnabled === true,
      "model picker",
    );
  },
});

for (const [scenario, harness, state] of CELLS)
  for (const width of WIDTHS) {
    const options = admitted(harness, state, width);
    if (!options) continue;
    test(
      `${harness} ${state} at ${width} is driven and witnessed on the rendered ${scenario} Chat`,
      { timeout: OPEN_MS + DRIVE_MS + SETTLE_MS * 4 },
      async (t) => {
        const capture = await openCapture(t, scenario, options);
        await PREPARE[scenario]?.(capture);
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

/**
 * Waits, as the capture drivers do, for a probe to hold; the composer and its
 * pickers can enable a moment after the projection loads on a slow runner.
 */
async function settled(probe, label = "control") {
  try {
    await waitForState(probe, Date.now() + SETTLE_MS);
  } catch (error) {
    if (error.message === "state-timeout")
      assert.fail(`${label} did not settle`);
    throw error;
  }
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
      // New chat is reachable without a navigation drawer at every width.
      for (const control of [NEW_CHAT, HARNESS_PICKER, ACCESS_OPTIONS])
        await settled(() => clickable(capture, control), control);
      const { selected } = await capture.observe();
      await settled(() => capture.evaluate(clickExpression(HARNESS_PICKER)));
      await settled(() => clickable(capture, harnessOption(selected)));
      await capture.evaluate(clickExpression(HARNESS_PICKER));
      await settled(() => capture.evaluate(clickExpression(ACCESS_OPTIONS)));
      for (const mode of ["readOnly", "workspaceWrite"])
        await settled(() => clickable(capture, accessModeOption(mode)), mode);
      await capture.evaluate(clickExpression(ACCESS_OPTIONS));
      await settled(() => capture.evaluate(COMPOSER_FILL));
      await settled(() => clickable(capture, SEND_MESSAGE));
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
      await settled(() => capture.evaluate(COMPOSER_FILL));
      await settled(() => clickable(capture, SEND_MESSAGE));
      await settled(() => capture.evaluate(mutation));
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
      // Mutate only once the picker is live, so the drift is what fails.
      await settled(() => clickable(capture, HARNESS_PICKER));
      await settled(() => capture.evaluate(mutation));
      await assert.rejects(
        drive(capture, options, Date.now() + 2_000),
        /state-timeout/u,
      );
      assert.equal((await reached(capture, options)).namedState, false);
    },
  );

test(
  "the context meter follows the measured toolbar fit: dropped when narrow, shown again when widened",
  { timeout: OPEN_MS + SETTLE_MS * 2 },
  async (t) => {
    // 320 is below every capture width: the toolbar needs the meter's column
    // there, so the fit drops it while the usage data stays known.
    const options = admitted("codex", "context-meter", 1888);
    const capture = await openCapture(t, "agent-composer-context-known", {
      ...options,
      width: 320,
      height: 1000,
    });
    await settled(
      async () => (await capture.observe()).meterFit === "dropped",
      "context meter dropped at 320",
    );
    const narrow = await capture.observe();
    assert.equal(narrow.visible.meter, false);
    assert.equal(narrow.controls.meter, false);
    assert.equal(narrow.controls.send, true);
    // Widening the same Chat refits the toolbar and the meter returns.
    await capture.page.setViewportSize({
      width: options.width,
      height: options.height,
    });
    await settled(async () => {
      const wide = await capture.observe();
      return wide.meterFit === "shown" && wide.controls.meter === true;
    }, "context meter shown again once widened");
  },
);

test(
  "a context meter hidden though its column fits fails the context-meter state",
  { timeout: OPEN_MS + SETTLE_MS * 2 },
  async (t) => {
    const options = admitted("codex", "context-meter", 1888);
    const capture = await openCapture(
      t,
      "agent-composer-context-known",
      options,
    );
    await settled(
      async () => (await capture.observe()).meterFit === "shown",
      "context meter shown",
    );
    // Hide the meter's column outside the fitter, so nothing justifies it.
    await capture.evaluate(
      `(()=>{const style=document.createElement('style');style.textContent='.composer__context-slot{display:none!important}';document.head.append(style);return true})()`,
    );
    await settled(
      async () => (await capture.observe()).meterFit === "hiddenThoughItFits",
      "meter hidden though it fits",
    );
    const predicates = namedState(
      options,
      await capture.observe(),
      capture.captureChat,
    );
    assert.equal(predicates.namedState, false);
  },
);

for (const hidden of ["display:none", "visibility:hidden"])
  test(
    `a meter with ${hidden} inside its laid-out slot fails the context-meter state`,
    { timeout: OPEN_MS + SETTLE_MS * 2 },
    async (t) => {
      const options = admitted("codex", "context-meter", 1888);
      const capture = await openCapture(
        t,
        "agent-composer-context-known",
        options,
      );
      await settled(async () => {
        const observation = await capture.observe();
        return observation.meterFit === "shown" && observation.visible.meter;
      }, "context meter shown");
      // The slot keeps its column; only the progressbar inside it is hidden.
      await capture.evaluate(
        `(()=>{const style=document.createElement('style');style.textContent='.composer__context-slot [role="progressbar"]{${hidden}!important}';document.head.append(style);return true})()`,
      );
      await settled(
        async () => (await capture.observe()).visible.meter === false,
        "meter hidden inside its slot",
      );
      const observation = await capture.observe();
      assert.equal(observation.meterFit, "shown");
      assert.equal(
        namedState(options, observation, capture.captureChat).namedState,
        false,
      );
    },
  );
