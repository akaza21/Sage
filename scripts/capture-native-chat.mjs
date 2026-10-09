import { writeFileSync, lstatSync } from "node:fs";
import { randomUUID } from "node:crypto";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { connectCdp } from "./source-functional-native-ide-cdp.mjs";
import { stagingPaths } from "./capture-native-chat-evidence.mjs";
import {
  parseArgs,
  requireNativeCase,
  validateProbe,
  validateDimensions,
  validatePng,
  admitTarget,
  waitForState,
  withOwnedCapture,
  readBoundedResponse,
} from "./capture-native-chat-contract.mjs";
import {
  OBSERVATION,
  IMAGE_ATTACHMENT,
  HARNESS,
  namedState,
  clickExpression,
  actionsFor,
  freshEmptyChat,
} from "./capture-native-chat-dom.mjs";
import {
  runTool,
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
} from "./capture-native-chat-source.mjs";
export {
  parseArgs,
  evidencePaths,
  validateProbe,
} from "./capture-native-chat-contract.mjs";

const workspace = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const idleStates = new Set([
  "new-chat",
  "composer-idle",
  "harness-menu",
  "add-menu",
]);

async function discover(port, deadline, child) {
  let selected;
  await waitForState(async () => {
    if (child.exitCode !== null || child.signalCode !== null || !child.pid)
      throw new Error("owned-app-exited");
    let response;
    try {
      response = await fetch("http://127.0.0.1:" + port + "/json", {
        signal: AbortSignal.timeout(1000),
      });
    } catch {
      return false;
    }
    const bytes = await readBoundedResponse(response);
    if (!response.ok || Buffer.byteLength(bytes) > 65536)
      throw new Error("cdp-discovery-bound");
    selected = admitTarget(JSON.parse(bytes), port);
    return true;
  }, deadline);
  return selected;
}
async function click(cdp, selector, deadline) {
  await waitForState(
    () =>
      cdp.evaluate(
        clickExpression(selector),
        Math.min(deadline, Date.now() + 5000),
      ),
    deadline,
  );
}
export async function prepareCaptureChat(cdp, harness, deadline) {
  const observe = () =>
    cdp.evaluate(OBSERVATION, Math.min(deadline, Date.now() + 5000));
  const previous = await observe();
  const compact = await cdp.evaluate(
    "Boolean(document.querySelector('button[aria-label=\"Open projects and chats\"]'))",
    Math.min(deadline, Date.now() + 5000),
  );
  if (compact)
    await click(cdp, 'button[aria-label="Open projects and chats"]', deadline);
  await click(cdp, 'button[aria-label="New task"]', deadline);
  await waitForState(
    async () => freshEmptyChat(previous, await observe()),
    deadline,
  );
  if (compact)
    await click(cdp, 'button[aria-label="Close projects and chats"]', deadline);
  await click(cdp, 'button[aria-label="Harness"]', deadline);
  await click(
    cdp,
    '[role="option"][data-option-id="' + HARNESS[harness].id + '"]',
    deadline,
  );
  await waitForState(
    async () => (await observe()).selected === HARNESS[harness].id,
    deadline,
  );
}
async function stableLayout(cdp, deadline) {
  await cdp.evaluate(
    "(async()=>{await document.fonts.ready;await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));return true})()",
    Math.min(deadline, Date.now() + 10000),
  );
  let prior = null,
    consecutive = 0;
  await waitForState(async () => {
    const observation = await cdp.evaluate(
      OBSERVATION,
      Math.min(deadline, Date.now() + 5000),
    );
    const signature = JSON.stringify([
      observation.css,
      observation.elements.map(({ kind, x, y, width, height }) => ({
        kind,
        x,
        y,
        width,
        height,
      })),
    ]);
    consecutive = signature === prior ? consecutive + 1 : 0;
    prior = signature;
    return consecutive >= 3;
  }, deadline);
}
function safeDestination(file) {
  try {
    if (!lstatSync(file).isFile() || lstatSync(file).isSymbolicLink())
      throw new Error("evidence-reparse");
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
}

export async function capture(options) {
  const run = randomUUID();
  const { directory, ...output } = stagingPaths(workspace, run, options);
  ensurePlainDirectory(directory);
  requirePlain(directory, true);
  for (const file of Object.values(output)) safeDestination(file);
  let child, viteChild, cdp, roots, nativeBounds, webviewVersion;
  const started = Date.now(),
    deadline = started + 90000;
  try {
    if (process.platform !== "win32") throw new Error("windows-required");
    requireNativeCase(options.harness, options.state);
    if (options.caseId !== options.harness + "-" + options.state)
      throw new Error("case-identity");
    if (typeof globalThis.WebSocket !== "function")
      throw new Error("node-websocket-unavailable");
    const source = await cleanSource(workspace);
    viteChild = await launchOwnedVite(workspace);
    await waitForVite(viteChild);
    const servedAsset = await servedAssetHash();
    assertOwnedVite(viteChild);
    const debugDir = path.join(
      process.env.CARGO_TARGET_DIR ||
        path.join(workspace, "src-tauri", "target"),
      "debug",
    );
    const build = executableIdentity(path.join(debugDir, "sage.exe"));
    const fixture = executableIdentity(
      path.join(debugDir, "acp-adversarial-fixture.exe"),
    );
    roots = freshRoots(workspace);
    const port = await reservePort();
    child = launchOwned(build, fixture, roots, options, port);
    const captured = await withOwnedCapture(
      async () => {
        const target = await discover(port, deadline, child);
        nativeBounds = JSON.parse(
          await runTool("powershell.exe", [
            "-NoProfile",
            "-NonInteractive",
            "-File",
            path.join(workspace, "scripts", "capture-native-chat-window.ps1"),
            "-ProcessId",
            String(child.pid),
            "-Width",
            String(options.width),
            "-Port",
            String(port),
          ]),
        );
        webviewVersion = nativeBounds.webviewVersion;
        nativeBounds = {
          outer: nativeBounds.outer,
          client: nativeBounds.client,
        };
        cdp = await connectCdp({
          target,
          deadline,
          openDeadline: Date.now() + 10000,
          WebSocketImpl: globalThis.WebSocket,
        });
        const evaluate = (expression) =>
          cdp.evaluate(expression, Math.min(deadline, Date.now() + 5000));
        await waitForState(
          () =>
            evaluate(
              "Boolean(document.querySelector('button[aria-label=\"Harness\"]'))",
            ),
          deadline,
        );
        let observation = await evaluate(OBSERVATION);
        validateDimensions(
          { ...nativeBounds, css: observation.css },
          options.width,
          options.height,
          observation.dpr,
        );
        await prepareCaptureChat(cdp, options.harness, deadline);
        const submit = async () => {
          await waitForState(
            () =>
              evaluate(
                '(()=>{const input=document.querySelector(\'textarea[aria-label="Message agent"]\');if(!input || input.disabled)return false;Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,"value").set.call(input,"Inspect the fixed synthetic native replay example.");input.dispatchEvent(new Event("input",{bubbles:true}));return true})()',
              ),
            deadline,
          );
          await click(cdp, 'button[aria-label="Send message"]', deadline);
        };
        if (options.state.startsWith("approval-")) {
          await submit();
          await waitForState(
            async () => (await evaluate(OBSERVATION)).state === "completed",
            deadline,
          );
          await click(cdp, 'button[aria-label="Composer options"]', deadline);
          await click(cdp, 'button[aria-label="Permission mode"]', deadline);
          await click(cdp, '[role="option"][data-option-id="ask"]', deadline);
          await click(cdp, 'button[aria-label="Composer options"]', deadline);
        }
        if (!idleStates.has(options.state)) {
          await submit();
          await waitForState(
            async () => (await evaluate(OBSERVATION)).turnId !== "none",
            deadline,
          );
        }
        if (options.state === "jump-to-latest")
          await evaluate(
            '(()=>{const transcript=document.querySelector(".message-list");if(!transcript)return false;transcript.scrollTop=0;transcript.dispatchEvent(new Event("scroll"));return true})()',
          );
        if (options.state === "attachment-chip") {
          await waitForState(
            async () => (await evaluate(OBSERVATION)).state === "completed",
            deadline,
          );
          await waitForState(() => evaluate(IMAGE_ATTACHMENT), deadline);
        }
        for (const selector of actionsFor(options.state, options.harness))
          await click(cdp, selector, deadline);
        await waitForState(async () => {
          observation = await evaluate(OBSERVATION);
          const predicates = namedState(options, observation);
          return (
            predicates.binding && predicates.selected && predicates.namedState
          );
        }, deadline);
        await stableLayout(cdp, deadline);
        observation = await evaluate(OBSERVATION);
        validateDimensions(
          { ...nativeBounds, css: observation.css },
          options.width,
          options.height,
          observation.dpr,
        );
        const probe = validateProbe({
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
          timing: { elapsedMs: Date.now() - started },
          verdict: "captured",
        });
        const screenshot = Buffer.from(
          await cdp.screenshot(Math.min(deadline, Date.now() + 10000)),
          "base64",
        );
        validatePng(screenshot, observation.css);
        const final = await evaluate(OBSERVATION);
        if (
          final.dpr !== 1 ||
          final.css.width !== observation.css.width ||
          final.css.height !== observation.css.height ||
          !Object.values(namedState(options, final)).every(Boolean)
        )
          throw new Error("capture-state-drift");
        return { screenshot, probe };
      },
      async () => {
        if (cdp) await cdp.close(Date.now() + 5000);
      },
      async () => {
        await stopOwned(child);
        cleanupRoots(roots);
        roots = undefined;
      },
    );
    await verifySource(workspace, source);
    assertOwnedVite(viteChild);
    if ((await servedAssetHash()) !== servedAsset)
      throw new Error("served-asset-drift");
    assertOwnedVite(viteChild);
    writeFileSync(output.png, captured.screenshot);
    writeFileSync(output.probe, JSON.stringify(captured.probe, null, 2) + "\n");
    return {
      verdict: "captured",
      run,
      directory: path.relative(workspace, directory),
      png: path.basename(output.png),
    };
  } catch (error) {
    const reason = /^[a-z0-9-]{1,80}$/u.test(error.message)
      ? error.message
      : "capture-operation-blocked";
    const blocked = {
      schemaVersion: 1,
      caseId: options.caseId,
      harness: options.harness,
      state: options.state,
      width: options.width,
      verdict: "blocked",
      reason,
    };
    if (nativeBounds) blocked.nativeBounds = nativeBounds;
    writeFileSync(output.probe, JSON.stringify(blocked, null, 2) + "\n");
    return { ...blocked, run };
  } finally {
    try {
      if (child) await stopOwned(child);
    } finally {
      try {
        if (viteChild) await stopOwned(viteChild);
      } finally {
        cleanupRoots(roots);
      }
    }
  }
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
