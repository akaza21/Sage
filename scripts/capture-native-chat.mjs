import { randomUUID } from "node:crypto";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { openOwnedCapture } from "./capture-native-chat-lifecycle.mjs";
import {
  stagingPaths,
  safeDestination,
  persistCapture,
} from "./capture-native-chat-evidence.mjs";
import {
  parseArgs,
  requireNativeCase,
  validateProbe,
  validateDimensions,
  validatePng,
  waitForState,
  withOwnedCapture,
} from "./capture-native-chat-contract.mjs";
import {
  observationFor,
  AGENT_MODE,
  IMAGE_ATTACHMENT,
  HARNESS,
  IDLE_CAPTURE_STATES,
  namedState,
  openExpression,
  isDisclosure,
  isLiveTurnAction,
  scopeToTurn,
  SETTLED_TURN_STATES,
  actionsFor,
} from "./capture-native-chat-dom.mjs";
import {
  requirePlain,
  ensurePlainDirectory,
  executableIdentity,
  reservePort,
  freshRoots,
  launchOwned,
  stopOwned,
  cleanupRoots,
} from "./capture-native-chat-process.mjs";
import {
  cleanSource,
  verifySource,
  launchOwnedVite,
  waitForVite,
  assertOwnedVite,
  servedAssetHash,
  COLD_ASSET_TIMEOUT_MS,
  buildCheckout,
} from "./capture-native-chat-source.mjs";
import {
  click,
  stableLayout,
  clickReadyTurn,
  captureTiming,
  captureStep,
  submitTurn,
  cleanupCapture,
  prepareCaptureChat,
  waitForComposerIdle,
  scrollCaptureTranscript,
  retainCaptureError,
} from "./capture-native-chat-drive.mjs";
export { prepareCaptureChat } from "./capture-native-chat-drive.mjs";
export {
  parseArgs,
  evidencePaths,
  validateProbe,
} from "./capture-native-chat-contract.mjs";

const workspace = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
export async function capture(options) {
  const timing = captureTiming();
  timing.record("capture-start");
  const run = randomUUID();
  const { directory, ...output } = stagingPaths(workspace, run, options);
  ensurePlainDirectory(directory);
  requirePlain(directory, true);
  for (const file of Object.values(output)) safeDestination(file);
  let child,
    ownedConnection,
    viteChild,
    cdp,
    roots,
    nativeBounds,
    webviewVersion,
    captured,
    captureError;
  let started = Date.now(),
    deadline = started + 90000;
  try {
    if (process.platform !== "win32") throw new Error("windows-required");
    requireNativeCase(options.harness, options.state);
    if (options.caseId !== options.harness + "-" + options.state)
      throw new Error("case-identity");
    if (typeof globalThis.WebSocket !== "function")
      throw new Error("node-websocket-unavailable");
    const source = await cleanSource(workspace);
    timing.record("source-clean");
    viteChild = await launchOwnedVite(workspace);
    await waitForVite(viteChild);
    const servedAsset = await servedAssetHash(fetch, COLD_ASSET_TIMEOUT_MS);
    timing.record("served-asset-ready");
    assertOwnedVite(viteChild);
    started = Date.now();
    deadline = started + 90000;
    const debugDir = path.join(
      process.env.CARGO_TARGET_DIR ||
        path.join(workspace, "src-tauri", "target"),
      "debug",
    );
    const build = executableIdentity(path.join(debugDir, "sage.exe"));
    const fixture = executableIdentity(
      path.join(debugDir, "acp-adversarial-fixture.exe"),
    );
    roots = freshRoots(
      await buildCheckout(workspace, process.env.NATIVE_CAPTURE_BUILD_CHECKOUT),
    );
    const port = await reservePort();
    child = launchOwned(build, fixture, roots, options, port);
    timing.record("app-launched");
    captured = await withOwnedCapture(
      async () => {
        ownedConnection = await openOwnedCapture(
          child,
          port,
          deadline,
          options,
          workspace,
          timing.record,
        );
        ({ cdp, nativeBounds, webviewVersion } = ownedConnection);
        const OBSERVATION = observationFor(HARNESS[options.harness].binding);
        const clickCapture = (selector) =>
          click(cdp, selector, deadline, timing.record);
        const evaluate = (expression) =>
          cdp.evaluate(expression, Math.min(deadline, Date.now() + 5000));
        await waitForState(() => evaluate(AGENT_MODE), deadline);
        await waitForState(
          () =>
            evaluate(
              "Boolean(document.querySelector('.agent-chat-pane--focused button[aria-label=\"Harness\"]'))",
            ),
          deadline,
        );
        let observation = await evaluate(OBSERVATION);
        timing.record("window-ready", observation);
        validateDimensions(
          { ...nativeBounds, css: observation.css },
          options.width,
          options.height,
          observation.dpr,
        );
        await prepareCaptureChat(cdp, options.harness, deadline, timing.record);
        let submittedTurnId;
        const submit = async () => {
          submittedTurnId = await submitTurn(
            evaluate,
            OBSERVATION,
            clickCapture,
            HARNESS[options.harness].binding,
            deadline,
            timing.record,
          );
        };
        if (options.state.startsWith("approval-")) {
          await submit();
          await waitForState(
            async () => (await evaluate(OBSERVATION)).state === "completed",
            deadline,
          );
          await clickCapture('button[aria-label="Composer options"]');
          await clickCapture('button[aria-label="Permission mode"]');
          await clickCapture('[role="option"][data-option-id="ask"]');
          await clickCapture('button[aria-label="Composer options"]');
        }
        if (!IDLE_CAPTURE_STATES.includes(options.state)) await submit();
        if (options.state === "jump-to-latest")
          await scrollCaptureTranscript(
            evaluate,
            OBSERVATION,
            submittedTurnId,
            HARNESS[options.harness].binding,
            deadline,
          );
        if (options.state === "attachment-chip") {
          await waitForState(
            async () => (await evaluate(OBSERVATION)).state === "completed",
            deadline,
          );
          await waitForState(() => evaluate(IMAGE_ATTACHMENT), deadline);
        }
        if (options.state === "model-menu")
          await waitForComposerIdle(
            evaluate,
            OBSERVATION,
            submittedTurnId,
            HARNESS[options.harness].binding,
            deadline,
            timing.record,
          );
        for (const action of actionsFor(options.state, options.harness)) {
          let latest = await evaluate(OBSERVATION);
          if (isLiveTurnAction(action)) {
            await clickReadyTurn(
              evaluate,
              OBSERVATION,
              scopeToTurn(action, submittedTurnId),
              submittedTurnId,
              HARNESS[options.harness].binding,
              deadline,
              timing.record,
            );
            continue;
          }
          timing.record("action-start", latest);
          if (!isDisclosure(action)) {
            await clickCapture(scopeToTurn(action, latest.turnId));
            timing.record("action-clicked", latest);
            continue;
          }
          await waitForState(async () => {
            latest = await evaluate(OBSERVATION);
            return SETTLED_TURN_STATES.includes(latest.state);
          }, deadline);
          const selector = scopeToTurn(action, latest.turnId);
          await waitForState(
            () => evaluate(openExpression(selector)),
            deadline,
          );
          timing.record("disclosure-opened", latest);
        }
        await waitForState(async () => {
          observation = await evaluate(OBSERVATION);
          const predicates = namedState(options, observation);
          return (
            predicates.binding && predicates.selected && predicates.namedState
          );
        }, deadline);
        await stableLayout(cdp, deadline, OBSERVATION);
        observation = await evaluate(OBSERVATION);
        timing.record("layout-stable", observation);
        validateDimensions(
          { ...nativeBounds, css: observation.css },
          options.width,
          options.height,
          observation.dpr,
        );
        const probe = {
          schemaVersion: 1,
          caseId: options.caseId,
          bindingId: observation.bindingId,
          turnId: observation.turnId,
          hashes: {
            build: build.sha256,
            fixture: fixture.sha256,
            source,
            servedAsset,
          },
          protocol: HARNESS[options.harness].protocol,
          webviewVersion,
          dpr: observation.dpr,
          dimensions: { ...nativeBounds, css: observation.css },
          predicates: namedState(options, observation),
          elements: observation.elements,
          controls: observation.controls,
          overflow: observation.overflow,
          timing: { elapsedMs: Date.now() - started, steps: timing.steps },
          verdict: "captured",
        };
        timing.record("screenshot-start", observation);
        const screenshot = Buffer.from(
          await cdp.screenshot(Math.min(deadline, Date.now() + 10000)),
          "base64",
        );
        validatePng(screenshot, options);
        timing.record("screenshot-captured", observation);
        const final = await evaluate(OBSERVATION);
        if (
          final.dpr !== 1 ||
          final.css.width !== observation.css.width ||
          final.css.height !== observation.css.height ||
          !Object.values(namedState(options, final)).every(Boolean)
        )
          throw new Error("capture-state-drift");
        timing.record("capture-verified", final);
        probe.timing.elapsedMs = Date.now() - started;
        return { screenshot, probe: validateProbe(probe) };
      },
      async () => {
        const closeError = await cleanupCapture(timing.record, [
          {
            step: "cdp-close",
            action: cdp ? () => cdp.close(Date.now() + 5000) : null,
          },
        ]);
        captureError = retainCaptureError(captureError, closeError);
        if (!captureError) cdp = undefined;
      },
      async () => {
        ownedConnection?.dispose();
        await captureStep(timing.record, "app-stop", () => stopOwned(child));
        child = undefined;
        await captureStep(timing.record, "roots-cleanup", () =>
          cleanupRoots(roots),
        );
        roots = undefined;
      },
    );
    await verifySource(workspace, source);
    timing.record("source-reverified");
    assertOwnedVite(viteChild);
    if ((await servedAssetHash()) !== servedAsset)
      throw new Error("served-asset-drift");
    assertOwnedVite(viteChild);
    timing.record("served-asset-reverified");
  } catch (error) {
    captureError = retainCaptureError(error, captureError);
  } finally {
    const cleanupError = await cleanupCapture(timing.record, [
      {
        step: "cdp-close",
        action: cdp ? () => cdp.close(Date.now() + 5000) : null,
      },
      { step: "app-stop", action: child ? () => stopOwned(child) : null },
      {
        step: "vite-stop",
        action: viteChild ? () => stopOwned(viteChild) : null,
      },
      {
        step: "roots-cleanup",
        action: roots ? () => cleanupRoots(roots) : null,
      },
    ]);
    captureError = retainCaptureError(captureError, cleanupError);
  }
  return persistCapture({
    output,
    options,
    run,
    directory: path.relative(workspace, directory),
    timing,
    elapsedMs: Date.now() - started,
    nativeBounds,
    captured,
    error: captureError,
  });
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href
) {
  try {
    const result = await capture(parseArgs(process.argv.slice(2)));
    console.log(JSON.stringify(result));
    if (result.verdict !== "captured") process.exitCode = 1;
  } catch (error) {
    const reason = /^[a-z0-9-]{1,80}$/u.test(error.message)
      ? error.message
      : "capture-operation-blocked";
    console.error("native-chat-capture-blocked: " + reason);
    process.exitCode = 1;
  }
}
