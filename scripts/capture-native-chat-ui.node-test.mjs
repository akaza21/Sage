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
  AGENT_MODE,
  COMPOSER_FILL,
  HARNESS,
  NEW_TASK,
  accessModeOption,
  namedState,
  observationFor,
} from "./capture-native-chat-dom.mjs";
import { runCaptureActions } from "./capture-native-chat-drive.mjs";
import { presentExpression } from "./capture-native-chat-parity-drive.mjs";
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

function reserveFreePort() {
  return new Promise((done, fail) => {
    const probe = createServer();
    probe.once("error", fail);
    probe.listen(0, "127.0.0.1", () => {
      const { port } = probe.address();
      probe.close(() => done(port));
    });
  });
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
  const url = `http://127.0.0.1:${await reserveFreePort()}/`;
  const cdpPort = await reserveFreePort();
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
  await lane.browser.close();
  assert.equal(await settleDevServer(lane.server), true);
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
      const response = await fetch(`http://127.0.0.1:${lane.cdpPort}/json`);
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

for (const width of WIDTHS)
  test(
    `the prepare and submit controls resolve exactly once at ${width}`,
    { timeout: OPEN_MS + DRIVE_MS },
    async (t) => {
      const options = admitted("codex", "access-menu", width);
      const capture = await openCapture(t, "agent-single", options);
      // New task is reachable without a navigation drawer at every width.
      assert.equal(await capture.evaluate(presentExpression(NEW_TASK)), true);
      await drive(capture, options);
      for (const mode of ["readOnly", "workspaceWrite"])
        assert.equal(
          await capture.evaluate(presentExpression(accessModeOption(mode))),
          true,
          mode,
        );
      assert.equal(await capture.evaluate(COMPOSER_FILL), true);
      await waitForState(
        async () => (await capture.observe()).controls.send === true,
        Date.now() + SETTLE_MS,
      );
    },
  );

test(
  "a renamed capture control fails the lane instead of driving another control",
  { timeout: OPEN_MS + DRIVE_MS },
  async (t) => {
    const options = admitted("codex", "harness-menu", 1888);
    const capture = await openCapture(t, "agent-single", options);
    assert.equal(
      await capture.evaluate(
        `(()=>{const picker=document.querySelector('.agent-chat-pane--focused button[aria-label="Harness"]');picker?.setAttribute('aria-label','Agent');return Boolean(picker)})()`,
      ),
      true,
    );
    await assert.rejects(
      drive(capture, options, Date.now() + 2_000),
      /state-timeout/u,
    );
    assert.equal((await reached(capture, options)).namedState, false);
  },
);
