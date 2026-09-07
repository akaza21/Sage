import { spawn } from "node:child_process";
import { existsSync, lstatSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  attachCdp,
  decodeBoundedScreenshot,
  discoverTarget,
  hashFile,
  operationDeadline,
  revalidateAttachedPage,
  writeContainedBytes,
  writeContainedText,
  writePendingJourneyReceipt,
} from "./source-functional-native-ide-journey.mjs";
import {
  finalizeAcp04Collection,
  preflightCodex,
  revalidateCollectionIdentities,
} from "./source-functional-acp04-finalizer.mjs";
import {
  ACP04_ECHOES,
  runAcp04ModeA,
} from "./source-functional-acp04-live-probe.mjs";
import { waitUntil } from "./source-functional-acp04-observation.mjs";
import {
  createAv01PreviewReceipt,
  parseAv01PreviewArguments,
  validateAv01FixtureProcessAbsence,
  validateAv01FixtureProcessCapture,
} from "./source-functional-av01-preview-contract.mjs";
import { readAv01PreviewFixtureIdentity } from "./source-functional-av01-preview-fixture.mjs";

const MAX_SNAPSHOTS = 32;
const MAX_TEXT = 1024;

function fail(code) {
  throw new Error(`av01-preview-${code}`);
}

function bounded(value, maximum = MAX_TEXT) {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    Buffer.byteLength(value, "utf8") <= maximum &&
    [...value].every((character) => {
      const code = character.codePointAt(0);
      return code > 31 && code !== 127;
    })
  );
}

export const AV01_PREVIEW_UI = Object.freeze({
  openPreview:
    '(()=>{const panes=[...document.querySelectorAll(\'aside[aria-label="Agent artifacts"]\')];if(panes.length!==1)return false;const tabs=[...panes[0].querySelectorAll(\'[role="tab"][aria-label="Preview"]\')];if(tabs.length!==1||tabs[0].getAttribute("aria-selected")==="true")return tabs.length===1;tabs[0].click();return true})()',
  previewReady:
    "(()=>{const stacks=[...document.querySelectorAll('.agent-preview-stack[data-preview-listener-ready=\"true\"]')];return stacks.length===1})()",
  startEnabled:
    '(()=>{const groups=[...document.querySelectorAll(\'[role="group"][aria-label="Preview targets"]\')];if(groups.length!==1)return false;const buttons=[...groups[0].querySelectorAll("button")].filter(x=>x.textContent.trim()==="Start"&&!x.disabled);return buttons.length===1})()',
  clickStart:
    '(()=>{const groups=[...document.querySelectorAll(\'[role="group"][aria-label="Preview targets"]\')];if(groups.length!==1)return false;const buttons=[...groups[0].querySelectorAll("button")].filter(x=>x.textContent.trim()==="Start"&&!x.disabled);if(buttons.length!==1)return false;buttons[0].click();return true})()',
  selectFile:
    '(()=>{const panes=[...document.querySelectorAll(\'aside[aria-label="Agent artifacts"]\')];if(panes.length!==1)return false;const tabs=[...panes[0].querySelectorAll(\'[role="tab"][aria-label^="File "]\')];if(tabs.length!==1)return false;tabs[0].click();return true})()',
  fileSelected:
    '(()=>{const panes=[...document.querySelectorAll(\'aside[aria-label="Agent artifacts"]\')];if(panes.length!==1)return false;const tabs=[...panes[0].querySelectorAll(\'[role="tab"][aria-label^="File "][aria-selected="true"]\')];return tabs.length===1})()',
  collapse:
    "(()=>{const panes=[...document.querySelectorAll('aside[aria-label=\"Agent artifacts\"]')];if(panes.length!==1)return false;const buttons=[...panes[0].querySelectorAll('button[aria-label=\"Collapse files, changes, and preview\"]')];if(buttons.length!==1)return false;buttons[0].click();return true})()",
  reopen:
    "(()=>{const panes=[...document.querySelectorAll('aside[aria-label=\"Agent artifacts\"]')];if(panes.length!==1)return false;const buttons=[...panes[0].querySelectorAll('button[aria-label^=\"Open files, changes, and preview\"]')];if(buttons.length!==1)return false;buttons[0].click();return true})()",
  leaveAgent:
    '(()=>{const groups=[...document.querySelectorAll(\'[role="radiogroup"][aria-label="Window mode"]\')];if(groups.length!==1)return false;const editor=[...groups[0].querySelectorAll(\'input[name="workbench-window-mode"][value="editor"]\')];if(editor.length!==1)return false;editor[0].click();return true})()',
});

export function previewManualOriginAction(origin) {
  let parsed;
  try {
    parsed = new URL(origin);
  } catch {
    fail("manual-origin-invalid");
  }
  if (
    parsed.protocol !== "http:" ||
    parsed.hostname !== "127.0.0.1" ||
    !/^\d{1,5}$/u.test(parsed.port) ||
    Number(parsed.port) < 1 ||
    Number(parsed.port) > 65535 ||
    parsed.username ||
    parsed.password ||
    parsed.hash
  )
    fail("manual-origin-invalid");
  const href = parsed.href;
  return `(()=>{const panes=[...document.querySelectorAll('aside[aria-label="Agent artifacts"]')];if(panes.length!==1)return false;const inputs=[...panes[0].querySelectorAll('input[aria-label="Preview address"]')];if(inputs.length!==1)return false;const input=inputs[0],form=input.closest('form');if(!form)return false;const descriptor=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value');if(!descriptor?.set)return false;descriptor.set.call(input,${JSON.stringify(href)});input.dispatchEvent(new Event('input',{bubbles:true}));form.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));return true})()`;
}

function readFixtureManualOrigin(options) {
  const recordPath = path.join(
    options.fixture.path,
    "av01-preview-manual.json",
  );
  if (!existsSync(recordPath)) return null;
  const item = lstatSync(recordPath);
  if (item.isSymbolicLink() || !item.isFile() || item.size > 4096)
    fail("manual-origin-record-invalid");
  let record;
  try {
    record = JSON.parse(readFileSync(recordPath, "utf8"));
  } catch {
    fail("manual-origin-record-invalid");
  }
  if (
    !record ||
    typeof record !== "object" ||
    Array.isArray(record) ||
    Object.keys(record).sort().join("\0") !== "nonce\0origin\0schemaVersion" ||
    record.schemaVersion !== 1 ||
    typeof record.nonce !== "string" ||
    typeof record.origin !== "string" ||
    record.nonce !==
      readAv01PreviewFixtureIdentity(options.fixture.path, options.fixture.case)
        .nonce
  )
    fail("manual-origin-record-invalid");
  previewManualOriginAction(record.origin);
  return record.origin;
}

async function waitForFixtureManualOrigin(options, deadline) {
  return waitUntil(
    () => readFixtureManualOrigin(options),
    deadline,
    "manual-origin-record-timeout",
  );
}

export const AV01_PREVIEW_SNAPSHOT = `(()=>{const stacks=[...document.querySelectorAll('.agent-preview-stack')],panes=[...document.querySelectorAll('aside[aria-label="Agent artifacts"]')];if(stacks.length>1||panes.length!==1)return null;const stack=stacks[0]||null,group=stack?.querySelector('[role="group"][aria-label="Preview targets"]')||null,buttons=group?[...group.querySelectorAll('button')]:[],frames=stack?[...stack.querySelectorAll('iframe[title="Sage browser"]')]:[],notices=stack?[...stack.querySelectorAll('.resource-state')].map(x=>x.innerText.trim()).filter(Boolean):[];return {listenerReady:stack?.dataset.previewListenerReady||'false',phase:stack?.dataset.previewPhase||'none',targetIdentity:stack?.dataset.previewTargetIdentity||null,startEnabled:buttons.filter(x=>x.textContent.trim()==='Start'&&!x.disabled).length===1,stopVisible:buttons.filter(x=>x.textContent.trim()==='Stop'&&!x.disabled).length===1,frameUrls:frames.map(x=>x.getAttribute('src')||''),notices,collapsed:panes[0].classList.contains('agent-artifact-pane--collapsed')};})()`;

export function validatePreviewSnapshot(value) {
  if (
    !value ||
    typeof value !== "object" ||
    Array.isArray(value) ||
    Object.keys(value).sort().join("\0") !==
      [
        "collapsed",
        "frameUrls",
        "listenerReady",
        "notices",
        "phase",
        "startEnabled",
        "stopVisible",
        "targetIdentity",
      ].join("\0") ||
    !["true", "false"].includes(value.listenerReady) ||
    !bounded(value.phase, 64) ||
    (value.targetIdentity !== null && !bounded(value.targetIdentity, 512)) ||
    typeof value.startEnabled !== "boolean" ||
    typeof value.stopVisible !== "boolean" ||
    typeof value.collapsed !== "boolean" ||
    !Array.isArray(value.frameUrls) ||
    value.frameUrls.length > 1 ||
    !value.frameUrls.every((url) => bounded(url, 2048)) ||
    !Array.isArray(value.notices) ||
    value.notices.length > 4 ||
    !value.notices.every((notice) => bounded(notice, 4096))
  )
    fail("snapshot-invalid");
  return Object.freeze({
    ...value,
    frameUrls: [...value.frameUrls],
    notices: [...value.notices],
  });
}

async function previewSnapshot(cdp, deadline) {
  return validatePreviewSnapshot(
    await cdp.evaluate(AV01_PREVIEW_SNAPSHOT, operationDeadline(deadline)),
  );
}

async function waitForSnapshot(cdp, deadline, predicate, code) {
  return waitUntil(
    async () => {
      const snapshot = await previewSnapshot(cdp, deadline);
      return predicate(snapshot) ? snapshot : null;
    },
    deadline,
    code,
  );
}

async function exactAction(cdp, expression, deadline, code) {
  if ((await cdp.evaluate(expression, operationDeadline(deadline))) !== true)
    fail(code);
}

async function openPreview(cdp, deadline) {
  await exactAction(
    cdp,
    AV01_PREVIEW_UI.openPreview,
    deadline,
    "preview-tab-ambiguous",
  );
  await waitUntil(
    () =>
      cdp.evaluate(AV01_PREVIEW_UI.previewReady, operationDeadline(deadline)),
    deadline,
    "listener-ready-timeout",
  );
  await waitUntil(
    () =>
      cdp.evaluate(AV01_PREVIEW_UI.startEnabled, operationDeadline(deadline)),
    deadline,
    "discovered-start-timeout",
  );
  return previewSnapshot(cdp, deadline);
}

async function proveSleep(cdp, active, deadline) {
  await exactAction(
    cdp,
    AV01_PREVIEW_UI.selectFile,
    deadline,
    "file-tab-ambiguous",
  );
  await waitUntil(
    () =>
      cdp.evaluate(AV01_PREVIEW_UI.fileSelected, operationDeadline(deadline)),
    deadline,
    "file-tab-timeout",
  );
  await exactAction(
    cdp,
    AV01_PREVIEW_UI.openPreview,
    deadline,
    "preview-return-ambiguous",
  );
  await waitForSnapshot(
    cdp,
    deadline,
    (snapshot) => snapshot.frameUrls[0] === active.frameUrls[0],
    "same-owner-preview-return-timeout",
  );
  await exactAction(
    cdp,
    AV01_PREVIEW_UI.collapse,
    deadline,
    "collapse-ambiguous",
  );
  await waitForSnapshot(
    cdp,
    deadline,
    (snapshot) => snapshot.collapsed,
    "collapse-timeout",
  );
  await exactAction(cdp, AV01_PREVIEW_UI.reopen, deadline, "reopen-ambiguous");
  await waitForSnapshot(
    cdp,
    deadline,
    (snapshot) =>
      !snapshot.collapsed && snapshot.frameUrls[0] === active.frameUrls[0],
    "same-owner-reopen-timeout",
  );
}

export function decodeAv01FixtureProcessHelperOutput(stdout, mode, expected) {
  if (typeof stdout !== "string" || Buffer.byteLength(stdout) > 8192)
    fail("fixture-process-json");
  for (const name of ["schemaVersion", "processId", "sageProcessId"]) {
    const matches = [
      ...stdout.matchAll(new RegExp(`"${name}"\\s*:\\s*([^,}]+)`, "gu")),
    ];
    if (matches.length !== 1 || !/^[1-9]\d*$/u.test(matches[0][1].trim()))
      fail("fixture-process-json");
  }
  let value;
  try {
    value = JSON.parse(stdout);
  } catch {
    fail("fixture-process-json");
  }
  try {
    return mode === "capture"
      ? validateAv01FixtureProcessCapture(value, expected)
      : validateAv01FixtureProcessAbsence(value, expected);
  } catch {
    fail("fixture-process-json");
  }
}

export async function runFixtureProcessHelper(
  options,
  mode,
  identity,
  dependencies = {},
) {
  const platform = dependencies.platform ?? process.platform;
  if (platform !== "win32") fail("fixture-process-platform");
  const fixtureIdentity = (
    dependencies.readFixtureIdentity ?? readAv01PreviewFixtureIdentity
  )(options.fixture.path, options.fixture.case);
  const expected =
    mode === "capture"
      ? {
          nonce: fixtureIdentity.nonce,
          sageProcessId: options.sageProcess.processId,
        }
      : identity;
  if (!expected) fail("fixture-process-identity-missing");
  let runtimeIdentity;
  try {
    runtimeIdentity =
      dependencies.runtimeIdentity ??
      hashFile(options.powershell.path, 256 * 1024 * 1024);
  } catch {
    fail("fixture-process-runtime-invalid");
  }
  if (
    runtimeIdentity.bytes !== options.powershell.bytes ||
    runtimeIdentity.sha256 !== options.powershell.sha256
  )
    fail("fixture-process-runtime-invalid");
  const argumentsFor = [
    "-NoProfile",
    "-ExecutionPolicy",
    "Bypass",
    "-File",
    options.fixture.processHelper,
    "-Mode",
    mode,
    "-FixtureRoot",
    options.fixture.path,
    "-StartupPath",
    path.join(options.fixture.path, "av01-preview-startup.json"),
    "-Nonce",
    fixtureIdentity.nonce,
    "-SageProcessId",
    String(options.sageProcess.processId),
    "-SageCreationDate",
    options.sageProcess.creationDate,
    "-SageExecutablePath",
    options.executable.path,
  ];
  if (identity)
    argumentsFor.push(
      "-IdentityJson",
      Buffer.from(JSON.stringify(identity)).toString("base64"),
    );
  const launch = dependencies.spawn ?? spawn;
  const timeoutMs = dependencies.timeoutMs ?? 5_000;
  return new Promise((resolve, reject) => {
    const child = launch(options.powershell.path, argumentsFor, {
      cwd: path.dirname(options.powershell.path),
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
    });
    let stdout = "";
    let stderr = "";
    let settled = false;
    let stdoutListener;
    let stderrListener;
    const settle = (callback) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (stdoutListener) child.stdout.off("data", stdoutListener);
      if (stderrListener) child.stderr.off("data", stderrListener);
      callback();
    };
    const timer = setTimeout(() => {
      child.kill();
      settle(() => reject(new Error("av01-preview-fixture-process-timeout")));
    }, timeoutMs);
    const appendBounded = (current, chunk) => {
      const bytes = Buffer.isBuffer(chunk)
        ? chunk.byteLength
        : Buffer.byteLength(String(chunk), "utf8");
      if (Buffer.byteLength(current) + bytes > 8192) return null;
      return (
        current +
        (Buffer.isBuffer(chunk) ? chunk.toString("utf8") : String(chunk))
      );
    };
    stdoutListener = (chunk) => {
      if (settled) return;
      const next = appendBounded(stdout, chunk);
      if (next === null) {
        child.kill();
        settle(() => reject(new Error("av01-preview-fixture-process-output")));
        return;
      }
      stdout = next;
    };
    stderrListener = (chunk) => {
      if (settled) return;
      const next = appendBounded(stderr, chunk);
      if (next === null) {
        child.kill();
        settle(() => reject(new Error("av01-preview-fixture-process-output")));
        return;
      }
      stderr = next;
    };
    child.stdout.on("data", stdoutListener);
    child.stderr.on("data", stderrListener);
    child.once("error", (error) => settle(() => reject(error)));
    child.once("close", (code) => {
      if (code !== 0)
        return settle(() =>
          reject(new Error(`av01-preview-fixture-process-${stderr || code}`)),
        );
      settle(() => {
        try {
          resolve(decodeAv01FixtureProcessHelperOutput(stdout, mode, expected));
        } catch (error) {
          reject(error);
        }
      });
    });
  });
}

function phaseNotice(snapshot, required) {
  return (
    snapshot.phase === required &&
    snapshot.notices.some((notice) => notice.includes("Preview"))
  );
}

export async function runAv01PreviewJourney(
  cdp,
  deadline,
  previewCase,
  hooks = {},
) {
  const ordered = [];
  const ready = await openPreview(cdp, deadline);
  if (!ready.startEnabled || ready.listenerReady !== "true")
    fail("listener-order-invalid");
  ordered.push({
    kind: "ready",
    at: new Date().toISOString(),
    snapshot: ready,
  });
  await exactAction(
    cdp,
    AV01_PREVIEW_UI.clickStart,
    deadline,
    "start-ambiguous",
  );
  const starting = await waitForSnapshot(
    cdp,
    deadline,
    (snapshot) =>
      phaseNotice(snapshot, "starting") && snapshot.frameUrls.length === 0,
    "starting-timeout",
  );
  ordered.push({
    kind: "starting",
    at: new Date().toISOString(),
    snapshot: starting,
  });
  let terminal;
  let fixtureProcess = null;
  let fixtureRetirement = null;
  if (previewCase === "starting-active") {
    terminal = await waitForSnapshot(
      cdp,
      deadline,
      (snapshot) =>
        snapshot.phase === "active" &&
        snapshot.stopVisible &&
        snapshot.frameUrls.length === 1,
      "active-timeout",
    );
    fixtureProcess = await hooks.captureFixtureProcess?.();
    await hooks.captureActiveScreenshot?.();
    await proveSleep(cdp, terminal, deadline);
    await exactAction(
      cdp,
      AV01_PREVIEW_UI.leaveAgent,
      deadline,
      "owner-disposal-ambiguous",
    );
    await waitUntil(
      () =>
        cdp.evaluate(
          "document.querySelectorAll('.agent-preview-stack').length===0",
          operationDeadline(deadline),
        ),
      deadline,
      "owner-disposal-timeout",
    );
    if (!fixtureProcess) fail("fixture-process-missing");
    fixtureRetirement = await hooks.assertFixtureRetired?.(fixtureProcess);
    if (!fixtureRetirement) fail("fixture-process-retirement-missing");
    ordered.push({ kind: "ownerDisposed", at: new Date().toISOString() });
  } else {
    const phase = previewCase === "health-failed" ? "healthFailed" : "exited";
    let manualOrigin = null;
    if (previewCase === "health-failed") {
      manualOrigin = await hooks.readManualOrigin?.();
      if (typeof manualOrigin !== "string") fail("manual-origin-missing");
      await exactAction(
        cdp,
        previewManualOriginAction(manualOrigin),
        deadline,
        "manual-origin-ambiguous",
      );
    }
    terminal = await waitForSnapshot(
      cdp,
      deadline,
      (snapshot) =>
        phaseNotice(snapshot, phase) &&
        (manualOrigin === null
          ? snapshot.frameUrls.length === 0
          : snapshot.frameUrls.length === 1 &&
            snapshot.frameUrls[0] === manualOrigin),
      `${phase}-timeout`,
    );
  }
  ordered.push({
    kind: terminal.phase,
    at: new Date().toISOString(),
    snapshot: terminal,
  });
  if (ordered.length > MAX_SNAPSHOTS) fail("observation-bound");
  return Object.freeze({
    case: previewCase,
    ordered,
    activeFrameUrl: terminal.frameUrls[0] ?? null,
    finalPhase: terminal.phase,
    manualOrigin:
      previewCase === "health-failed" ? (terminal.frameUrls[0] ?? null) : null,
    fixtureProcess,
    fixtureRetirement,
  });
}

function guardNativeLogs(options) {
  for (const logPath of [
    options.nativeLogs.stdout,
    options.nativeLogs.stderr,
  ]) {
    const item = existsSync(logPath) ? lstatSync(logPath) : null;
    if (
      !item ||
      item.isSymbolicLink() ||
      !item.isFile() ||
      item.size > 1024 * 1024
    )
      fail("native-log-bound");
  }
}

async function execute(options) {
  const deadline = Date.now() + options.deadlineMs;
  options = await preflightCodex(options, deadline);
  readAv01PreviewFixtureIdentity(options.fixture.path, options.fixture.case);
  const admitted = await discoverTarget(
    options.port,
    options.expectedOrigin,
    deadline,
  );
  const cdp = await attachCdp(admitted, deadline);
  try {
    await revalidateAttachedPage(options, cdp, admitted, deadline);
    const modeA = await runAcp04ModeA(cdp, deadline, ACP04_ECHOES, {
      codexVersion: options.codex.version,
      guard: () => guardNativeLogs(options),
    });
    let activeScreenshot = null;
    const preview = await runAv01PreviewJourney(
      cdp,
      deadline,
      options.fixture.case,
      {
        captureFixtureProcess: () =>
          runFixtureProcessHelper(options, "capture"),
        assertFixtureRetired: (identity) =>
          runFixtureProcessHelper(options, "absent", identity),
        readManualOrigin: () => waitForFixtureManualOrigin(options, deadline),
        captureActiveScreenshot: async () => {
          activeScreenshot = decodeBoundedScreenshot(
            await cdp.screenshot(operationDeadline(deadline)),
          );
        },
      },
    );
    const observationName = "av01-preview-observation.json";
    const screenshotName = "av01-preview.png";
    const result = {
      startedAt: modeA.startedAt,
      completedAt: new Date().toISOString(),
      modeA,
      preview,
    };
    writeContainedText(
      options.sourceRoot,
      options.output,
      observationName,
      `${JSON.stringify(result, null, 2)}\n`,
    );
    writeContainedBytes(
      options.sourceRoot,
      options.output,
      screenshotName,
      activeScreenshot ??
        decodeBoundedScreenshot(
          await cdp.screenshot(operationDeadline(deadline)),
        ),
    );
    const target = await revalidateAttachedPage(
      options,
      cdp,
      admitted,
      deadline,
    );
    return finalizeAcp04Collection({
      closeCdp: () => cdp.close(Date.now() + 5_000),
      revalidateIdentities: () =>
        revalidateCollectionIdentities(options, deadline),
      createReceipt: () =>
        createAv01PreviewReceipt(
          options,
          result,
          target,
          [observationName, screenshotName],
          cdp.stats,
        ),
      writePendingReceipt: (receipt) =>
        writePendingJourneyReceipt(
          options.sourceRoot,
          options.output,
          options.receiptName,
          receipt,
        ),
    });
  } catch (error) {
    try {
      await cdp.close(Date.now() + 5_000);
    } catch {
      /* primary error remains */
    }
    throw error;
  }
}

if (
  process.argv[1] &&
  path.resolve(fileURLToPath(import.meta.url)) === path.resolve(process.argv[1])
) {
  let options;
  try {
    options = parseAv01PreviewArguments(process.argv.slice(2));
    await execute(options);
  } catch (error) {
    try {
      if (options?.output && existsSync(options.output))
        writeContainedText(
          options.sourceRoot,
          options.output,
          "machine-failure.json",
          `${JSON.stringify({ schemaVersion: 1, completed: false, error: String(error?.message ?? error).slice(0, 1024) }, null, 2)}\n`,
        );
    } catch {
      /* unsafe roots never receive diagnostics */
    }
    console.error(String(error?.message ?? error).slice(0, 1024));
    process.exitCode = 1;
  }
}
