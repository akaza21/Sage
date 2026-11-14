import { randomUUID, createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { openOwnedCapture } from "./capture-native-chat-lifecycle.mjs";
import {
  stagingPaths,
  safeDestination,
  persistCapture,
} from "./capture-native-chat-evidence.mjs";
import {
  boundedEvaluate,
  retainCaptureError,
} from "./capture-native-chat-shared.mjs";
import {
  validateDimensions,
  validatePng,
} from "./capture-native-chat-contract.mjs";
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
  captureTiming,
  cleanupCapture,
  stableLayout,
} from "./capture-native-chat-drive.mjs";
import { cleanSource, verifySource } from "./capture-native-chat-source.mjs";
import {
  admitEditor,
  parseEditorArgs,
  validateEditorProbe,
  EDITOR_FILE,
  SEARCH_FILE,
} from "./capture-native-editor-catalog.mjs";
import { assertEditorState } from "./capture-native-editor-dom.mjs";
import { driveEditor } from "./capture-native-editor-drive.mjs";

const workspace = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
export const PACKAGED_PAGE = "http://tauri.localhost/";

export function seedEditorProject(root, project) {
  const digest = createHash("sha256");
  let midLine;
  for (const relative of [EDITOR_FILE, SEARCH_FILE]) {
    const source = requirePlain(path.join(root, relative));
    const identity = executableIdentity(source);
    if (identity.bytes > 1024 * 1024) throw new Error("editor-source-bound");
    const bytes = readFileSync(source);
    const destination = path.resolve(project, relative);
    if (!destination.startsWith(path.resolve(project) + path.sep))
      throw new Error("editor-seed-containment");
    ensurePlainDirectory(path.dirname(destination));
    writeFileSync(destination, bytes, { flag: "wx" });
    digest.update(relative).update(bytes);
    if (relative === EDITOR_FILE)
      midLine = Math.max(
        2,
        Math.floor(bytes.toString("utf8").split(/\r?\n/u).length / 2),
      );
  }
  return { midLine, sha256: digest.digest("hex") };
}

export async function captureEditorFrame({
  cdp,
  options,
  expression,
  deadline,
  identity,
  record = () => {},
}) {
  admitEditor(options);
  const evaluate = boundedEvaluate(cdp, deadline);
  await stableLayout(cdp, deadline, expression);
  const before = assertEditorState(await evaluate(expression));
  const dimensions = { ...identity.nativeBounds, css: before.css };
  validateDimensions(dimensions, options.width, options.height, before.dpr);
  const probe = validateEditorProbe({
    schemaVersion: 1,
    caseId: options.caseId,
    bindingId: "native-editor",
    turnId: "none",
    hashes: identity.hashes,
    protocol: "native-editor-1",
    webviewVersion: identity.webviewVersion,
    dpr: before.dpr,
    dimensions,
    predicates: before.predicates,
    assistantRendered: false,
    lazyFallbackVisible: before.lazyFallbackVisible,
    elements: before.elements,
    controls: before.controls,
    overflow: before.overflow,
    timing: { elapsedMs: 0, steps: [] },
    verdict: "captured",
  });
  record("editor-probe-verified");
  const screenshot = Buffer.from(
    await cdp.screenshot(Math.min(deadline, Date.now() + 10000)),
    "base64",
  );
  validatePng(screenshot, options);
  const after = assertEditorState(await evaluate(expression));
  validateDimensions(
    { ...identity.nativeBounds, css: after.css },
    options.width,
    options.height,
    after.dpr,
  );
  if (JSON.stringify(before) !== JSON.stringify(after))
    throw new Error("editor-state-drift");
  record("editor-screenshot-verified");
  return { screenshot, probe };
}

export async function captureEditor(options, root = workspace) {
  admitEditor(options);
  const timing = captureTiming();
  timing.record("capture-start");
  const started = Date.now(),
    deadline = started + 90000,
    run = randomUUID();
  const { directory, ...output } = stagingPaths(
    root,
    run,
    options,
    admitEditor,
  );
  ensurePlainDirectory(directory);
  for (const file of Object.values(output)) safeDestination(file);
  let roots, child, owned, captured, error;
  try {
    if (process.platform !== "win32") throw new Error("windows-required");
    if (typeof globalThis.WebSocket !== "function")
      throw new Error("node-websocket-unavailable");
    const source = await cleanSource(root);
    const build = executableIdentity(
      process.env.NATIVE_EDITOR_EXE ||
        path.join(
          process.env.CARGO_TARGET_DIR || path.join(root, "target"),
          "debug",
          "sage.exe",
        ),
    );
    roots = freshRoots(root);
    const seed = seedEditorProject(root, roots.project);
    const port = await reservePort();
    if ([1420, 1431].includes(port)) throw new Error("editor-port-forbidden");
    child = launchOwned(build, null, roots, options, port);
    timing.record("app-launched");
    owned = await openOwnedCapture(
      child,
      port,
      deadline,
      options,
      root,
      timing.record,
      PACKAGED_PAGE,
    );
    const { expression } = await driveEditor(
      owned.cdp,
      options,
      deadline,
      seed,
      timing.record,
    );
    captured = await captureEditorFrame({
      cdp: owned.cdp,
      options,
      expression,
      deadline,
      record: timing.record,
      identity: {
        nativeBounds: owned.nativeBounds,
        webviewVersion: owned.webviewVersion,
        hashes: {
          build: build.sha256,
          fixture: seed.sha256,
          source,
          servedAsset: build.sha256,
        },
      },
    });
    await verifySource(root, source);
    if (executableIdentity(build.path).sha256 !== build.sha256)
      throw new Error("editor-build-drift");
  } catch (failure) {
    error = failure;
  } finally {
    error = retainCaptureError(
      error,
      await cleanupCapture(timing.record, [
        {
          step: "cdp-close",
          action: owned ? () => owned.cdp.close(Date.now() + 5000) : null,
        },
        { step: "app-stop", action: child ? () => stopOwned(child) : null },
        {
          step: "roots-cleanup",
          action: roots
            ? () => {
                if (
                  child?.pid &&
                  child.exitCode === null &&
                  child.signalCode === null
                )
                  throw new Error("owned-app-still-running");
                return cleanupRoots(roots);
              }
            : null,
        },
      ]),
    );
    owned?.dispose();
  }
  return persistCapture(
    {
      output,
      options,
      run,
      directory: path.relative(root, directory),
      timing,
      elapsedMs: Date.now() - started,
      nativeBounds: owned?.nativeBounds,
      captured,
      error,
    },
    writeFileSync,
    validateEditorProbe,
  );
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href
) {
  try {
    const result = await captureEditor(parseEditorArgs(process.argv.slice(2)));
    console.log(JSON.stringify(result));
    if (result.verdict !== "captured") process.exitCode = 1;
  } catch (error) {
    console.error("native-editor-capture-blocked: " + error.message);
    process.exitCode = 1;
  }
}
