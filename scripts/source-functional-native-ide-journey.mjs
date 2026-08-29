import { createHash } from "node:crypto";
import {
  closeSync,
  existsSync,
  openSync,
  fstatSync,
  lstatSync,
  readFileSync,
  realpathSync,
  readSync,
  statSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { runBoundedTool } from "./source-functional-native-ide-bounded-tool.mjs";
import {
  createPlaywrightTransportReceipt,
  resolvePlaywrightTransport,
  runPlaywrightTool,
  validatePlaywrightTransportReceipt,
  validatePlaywrightTransport,
} from "./source-functional-native-ide-transport.mjs";

export {
  processIdentityHelper,
  runBoundedTool,
} from "./source-functional-native-ide-bounded-tool.mjs";
export {
  resolvePlaywrightTransport,
  runPlaywrightTool,
  validatePlaywrightTransport,
} from "./source-functional-native-ide-transport.mjs";

export const JOURNEY_STEPS = Object.freeze([
  "Project",
  "Explorer",
  "Search",
  "File",
  "edit-diagnostic",
  "save",
  "Source Control",
  "Diff",
]);

const SHA256 = /^[a-f0-9]{64}$/u;
const GIT_OBJECT = /^[a-f0-9]{40,64}$/u;
const LOOPBACK_HTTP_ORIGIN = /^http:\/\/127\.0\.0\.1:(?<port>\d{4,5})\/?$/u;
const MAX_ARTIFACT_BYTES = 8 * 1024 * 1024;
const MAX_EXECUTABLE_BYTES = 256 * 1024 * 1024;
const MAX_PLAYWRIGHT_RESULT_BYTES = 64 * 1024;
const MAX_PLAYWRIGHT_ENVELOPE_OVERHEAD_BYTES = 4096;
const MAX_SCREENSHOT_BASE64_CHARACTERS = 4 * Math.ceil(MAX_ARTIFACT_BYTES / 3);
const MAX_SCREENSHOT_RESULT_BYTES = MAX_SCREENSHOT_BASE64_CHARACTERS + 2;
const MAX_SCREENSHOT_OUTPUT_BYTES =
  MAX_SCREENSHOT_RESULT_BYTES + MAX_PLAYWRIGHT_ENVELOPE_OVERHEAD_BYTES;
const PLAYWRIGHT_RESULT_MARKER = "### Result\n";
const PLAYWRIGHT_CODE_MARKER = "\n### Ran Playwright code\n";
const PENDING_RECEIPT_NAME =
  /^machine-receipt\.pending(?:-[a-z0-9-]{1,32})?\.json$/u;

function fail(code) {
  throw new Error(`journey-${code}`);
}

function isAbsolutePath(value) {
  return (
    typeof value === "string" &&
    (path.isAbsolute(value) || path.win32.isAbsolute(value))
  );
}

function canonicalExpectedOrigin(value) {
  if (typeof value !== "string") return null;
  const match = LOOPBACK_HTTP_ORIGIN.exec(value);
  if (!match) return null;
  const port = Number(match.groups.port);
  if (!Number.isInteger(port) || port < 1024 || port > 65535) return null;
  return value.endsWith("/") ? value : `${value}/`;
}

function hashFile(filePath, maximumBytes = MAX_ARTIFACT_BYTES) {
  if (lstatSync(filePath).isSymbolicLink()) fail("artifact-invalid");
  const stats = statSync(filePath);
  if (!stats.isFile() || stats.size > maximumBytes) fail("artifact-invalid");
  const digest = createHash("sha256");
  const descriptor = openSync(filePath, "r");
  const chunk = Buffer.allocUnsafe(1024 * 1024);
  try {
    let count;
    while ((count = readSync(descriptor, chunk, 0, chunk.length, null)) > 0)
      digest.update(chunk.subarray(0, count));
  } finally {
    closeSync(descriptor);
  }
  return {
    bytes: stats.size,
    sha256: digest.digest("hex"),
  };
}

export function hashExecutable(filePath, deps = {}) {
  const before = lstatSync(filePath, { bigint: true });
  if (
    !before.isFile() ||
    before.isSymbolicLink() ||
    before.size > BigInt(MAX_EXECUTABLE_BYTES)
  )
    fail("executable-identity-invalid");
  const descriptor = openSync(filePath, "r");
  const opened = fstatSync(descriptor, { bigint: true });
  const digest = createHash("sha256");
  const chunk = Buffer.allocUnsafe(1024 * 1024);
  try {
    if (
      opened.dev !== before.dev ||
      opened.ino !== before.ino ||
      opened.size !== before.size ||
      opened.mtimeNs !== before.mtimeNs
    )
      fail("executable-identity-invalid");
    let count;
    while ((count = readSync(descriptor, chunk, 0, chunk.length, null)) > 0) {
      digest.update(chunk.subarray(0, count));
      deps.afterRead?.();
    }
    const openedAfter = fstatSync(descriptor, { bigint: true });
    const pathAfter = lstatSync(filePath, { bigint: true });
    for (const key of ["dev", "ino", "size", "mtimeNs"]) {
      if (openedAfter[key] !== opened[key] || pathAfter[key] !== opened[key])
        fail("executable-identity-mutated");
    }
  } finally {
    closeSync(descriptor);
  }
  return { bytes: Number(opened.size), sha256: digest.digest("hex") };
}

export function validateExecutableBinding(filePath, expected, deps = {}) {
  const actual = hashExecutable(filePath, deps);
  if (actual.bytes !== expected.bytes || actual.sha256 !== expected.sha256)
    fail("executable-binding-mismatch");
  return actual;
}

function exactKeys(value, keys) {
  return (
    value &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    Object.keys(value).sort().join("\0") === [...keys].sort().join("\0")
  );
}

export function parseJourneyArguments(argv) {
  if (!Array.isArray(argv) || argv.length % 2 !== 0) fail("arguments-invalid");
  const options = Object.create(null);
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (
      typeof key !== "string" ||
      !key.startsWith("--") ||
      typeof value !== "string" ||
      key in options
    )
      fail("arguments-invalid");
    options[key.slice(2)] = value;
  }
  const known = new Set([
    "port",
    "output",
    "source-commit",
    "source-tree",
    "executable",
    "fixture",
    "source-root",
    "playwright-cli",
    "executable-bytes",
    "executable-sha256",
    "expected-origin",
    "deadline-ms",
    "runtime-mode",
    "frontend-ready",
    "receipt-name",
  ]);
  if (Object.keys(options).some((key) => !known.has(key)))
    fail("arguments-invalid");
  const port = Number(options.port);
  const deadlineMs =
    options["deadline-ms"] === undefined
      ? 90_000
      : Number(options["deadline-ms"]);
  const executableBytes = Number(options["executable-bytes"]);
  const expectedOrigin = canonicalExpectedOrigin(options["expected-origin"]);
  if (
    !Number.isInteger(port) ||
    port < 1024 ||
    port > 65535 ||
    !Number.isInteger(deadlineMs) ||
    deadlineMs < 15_000 ||
    deadlineMs > 180_000 ||
    !GIT_OBJECT.test(options["source-commit"] ?? "") ||
    !GIT_OBJECT.test(options["source-tree"] ?? "") ||
    !isAbsolutePath(options.output) ||
    !isAbsolutePath(options.executable) ||
    !isAbsolutePath(options.fixture) ||
    !isAbsolutePath(options["source-root"]) ||
    !isAbsolutePath(options["playwright-cli"]) ||
    !Number.isSafeInteger(executableBytes) ||
    executableBytes <= 0 ||
    executableBytes > MAX_EXECUTABLE_BYTES ||
    !SHA256.test(options["executable-sha256"] ?? "") ||
    expectedOrigin === null ||
    options["runtime-mode"] !== "tauri-dev-split" ||
    options["frontend-ready"] !== "true" ||
    !PENDING_RECEIPT_NAME.test(options["receipt-name"] ?? "")
  ) {
    fail("arguments-invalid");
  }
  return Object.freeze({
    port,
    output: options.output,
    sourceCommit: options["source-commit"],
    sourceTree: options["source-tree"],
    executable: options.executable,
    fixture: options.fixture,
    sourceRoot: options["source-root"],
    playwrightCli: options["playwright-cli"],
    executableBytes,
    executableSha256: options["executable-sha256"],
    expectedOrigin,
    deadlineMs,
    runtimeMode: options["runtime-mode"],
    frontendReady: true,
    receiptName: options["receipt-name"],
    steps: JOURNEY_STEPS,
  });
}

function isWithin(root, candidate) {
  const relative = path.relative(realpathSync(root), realpathSync(candidate));
  return (
    relative !== "" &&
    !relative.startsWith(`..${path.sep}`) &&
    relative !== ".." &&
    !path.isAbsolute(relative)
  );
}

export function createJourneyReceipt({
  source,
  executable,
  playwrightTransport,
  fixture,
  cdp,
  expectedOrigin,
  runtime,
  milestones,
  artifacts,
  observations,
  evidenceRoot,
}) {
  let admittedPlaywrightTransport;
  try {
    admittedPlaywrightTransport =
      validatePlaywrightTransportReceipt(playwrightTransport);
  } catch {
    fail("receipt-invalid");
  }
  if (
    !exactKeys(source, ["commit", "tree"]) ||
    !GIT_OBJECT.test(source.commit) ||
    !GIT_OBJECT.test(source.tree) ||
    !exactKeys(executable, ["bytes", "path", "sha256"]) ||
    !isAbsolutePath(executable.path) ||
    !Number.isSafeInteger(executable.bytes) ||
    executable.bytes <= 0 ||
    !SHA256.test(executable.sha256) ||
    !exactKeys(fixture, ["path", "sha256"]) ||
    !isAbsolutePath(fixture.path) ||
    !SHA256.test(fixture.sha256) ||
    !exactKeys(cdp, ["port", "targetId", "origin"]) ||
    !Number.isInteger(cdp.port) ||
    cdp.port < 1024 ||
    cdp.port > 65535 ||
    typeof cdp.targetId !== "string" ||
    cdp.targetId.length < 1 ||
    cdp.targetId.length > 256 ||
    canonicalExpectedOrigin(expectedOrigin) !== expectedOrigin ||
    cdp.origin !== expectedOrigin ||
    !exactKeys(runtime, ["frontendReady", "mode"]) ||
    runtime.mode !== "tauri-dev-split" ||
    runtime.frontendReady !== true
  ) {
    fail("receipt-invalid");
  }
  if (
    !Array.isArray(milestones) ||
    milestones.length !== JOURNEY_STEPS.length ||
    !milestones.every(
      (milestone, index) =>
        exactKeys(milestone, ["ordinal", "step"]) &&
        milestone.ordinal === index + 1 &&
        milestone.step === JOURNEY_STEPS[index],
    )
  ) {
    fail("milestones-invalid");
  }
  if (
    !exactKeys(observations, ["diagnostic", "diff", "save"]) ||
    !exactKeys(observations.save, [
      "contentSha256",
      "diskSha256",
      "editorPath",
      "saveDisabled",
    ]) ||
    observations.save.editorPath !== "journey.ts" ||
    observations.save.saveDisabled !== true ||
    !SHA256.test(observations.save.contentSha256) ||
    !SHA256.test(observations.save.diskSha256) ||
    !exactKeys(observations.diagnostic, [
      "count",
      "messagesSha256",
      "resource",
    ]) ||
    observations.diagnostic.resource !== "journey.ts" ||
    !Number.isSafeInteger(observations.diagnostic.count) ||
    observations.diagnostic.count < 1 ||
    !SHA256.test(observations.diagnostic.messagesSha256) ||
    !exactKeys(observations.diff, ["contentSha256", "lineCount", "resource"]) ||
    observations.diff.resource !== "journey.ts" ||
    !Number.isSafeInteger(observations.diff.lineCount) ||
    observations.diff.lineCount < 1 ||
    !SHA256.test(observations.diff.contentSha256)
  )
    fail("observations-invalid");
  if (
    !Array.isArray(artifacts) ||
    artifacts.length < 2 ||
    artifacts.length > 32 ||
    !artifacts.every(
      (artifact) =>
        isAbsolutePath(artifact) &&
        existsSync(artifact) &&
        !lstatSync(artifact).isSymbolicLink() &&
        (!evidenceRoot || isWithin(evidenceRoot, artifact)),
    )
  ) {
    fail("artifact-invalid");
  }
  const names = new Set();
  const records = artifacts.map((artifact) => {
    const name = path.basename(artifact);
    if (!name || names.has(name)) fail("artifact-invalid");
    names.add(name);
    return { name, ...hashFile(artifact) };
  });
  return Object.freeze({
    schemaVersion: 3,
    completed: true,
    source: Object.freeze({ ...source }),
    sourceBinding: "collection-only",
    runtime: Object.freeze({ ...runtime }),
    executable: Object.freeze({
      bytes: executable.bytes,
      sha256: executable.sha256,
    }),
    playwrightTransport: admittedPlaywrightTransport,
    fixture: Object.freeze({ sha256: fixture.sha256 }),
    cdp: Object.freeze({ ...cdp }),
    observations: Object.freeze({
      save: Object.freeze({ ...observations.save }),
      diagnostic: Object.freeze({ ...observations.diagnostic }),
      diff: Object.freeze({ ...observations.diff }),
    }),
    milestones: Object.freeze(
      milestones.map((milestone) => Object.freeze({ ...milestone })),
    ),
    artifacts: Object.freeze(records.map((record) => Object.freeze(record))),
  });
}

function sleep(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

export function admitCdpTarget(targets, port, expectedOrigin) {
  const canonicalOrigin = canonicalExpectedOrigin(expectedOrigin);
  if (
    !Array.isArray(targets) ||
    targets.length > 32 ||
    canonicalOrigin === null
  )
    fail("cdp-targets-invalid");
  const originWithoutSlash = canonicalOrigin.slice(0, -1);
  const pages = targets.filter(
    (target) =>
      target?.type === "page" &&
      target?.title === "Sage" &&
      (target?.url === canonicalOrigin || target?.url === originWithoutSlash),
  );
  if (pages.length !== 1) fail(`cdp-page-count-${pages.length}`);
  validateCdpTarget(pages[0], port);
  return pages[0];
}

export function revalidateControlledCdpTarget(
  admittedTarget,
  currentTargets,
  controlledTarget,
  port,
  expectedOrigin,
) {
  const currentTarget = admitCdpTarget(currentTargets, port, expectedOrigin);
  if (
    currentTarget.id !== admittedTarget?.id ||
    currentTarget.webSocketDebuggerUrl !== admittedTarget?.webSocketDebuggerUrl
  )
    fail("cdp-target-churn");
  const canonicalOrigin = canonicalExpectedOrigin(expectedOrigin);
  const originWithoutSlash = canonicalOrigin?.slice(0, -1);
  if (
    !exactKeys(controlledTarget, ["id", "title", "type", "url"]) ||
    controlledTarget.id !== admittedTarget.id ||
    controlledTarget.type !== "page" ||
    controlledTarget.title !== "Sage" ||
    (controlledTarget.url !== canonicalOrigin &&
      controlledTarget.url !== originWithoutSlash)
  )
    fail("cdp-controlled-target-mismatch");
  return Object.freeze({
    id: controlledTarget.id,
    type: "page",
    title: "Sage",
    url: canonicalOrigin,
  });
}

async function readCdpTargets(port, deadline) {
  const remaining = deadline - Date.now();
  if (remaining <= 0) throw new Error("target discovery deadline elapsed");
  const response = await fetch(`http://127.0.0.1:${port}/json`, {
    signal: AbortSignal.timeout(remaining),
  });
  if (!response.ok) throw new Error(`status ${response.status}`);
  const body = await response.text();
  if (Buffer.byteLength(body, "utf8") > 64 * 1024)
    throw new Error("target discovery exceeded bound");
  const targets = JSON.parse(body);
  if (!Array.isArray(targets) || targets.length > 32)
    throw new Error("target discovery was invalid");
  return targets;
}

async function discoverTarget(port, expectedOrigin, deadline) {
  let lastError = "CDP endpoint did not respond";
  while (Date.now() < deadline) {
    try {
      const targets = await readCdpTargets(port, deadline);
      return admitCdpTarget(targets, port, expectedOrigin);
    } catch (error) {
      lastError = String(error?.message ?? error).slice(0, 512);
    }
    await sleep(Math.min(100, Math.max(0, deadline - Date.now())));
  }
  fail(`cdp-discovery-timeout:${lastError}`);
}

export function validateCdpTarget(target, port) {
  try {
    const socket = new URL(target?.webSocketDebuggerUrl);
    const encodedTargetId = encodeURIComponent(target?.id ?? "");
    const expectedSocket = `ws://127.0.0.1:${port}/devtools/page/${encodedTargetId}`;
    if (
      typeof target?.id !== "string" ||
      target.id.length < 1 ||
      target.id.length > 256 ||
      socket.protocol !== "ws:" ||
      socket.hostname !== "127.0.0.1" ||
      Number(socket.port) !== port ||
      socket.username !== "" ||
      socket.password !== "" ||
      socket.pathname !== `/devtools/page/${encodedTargetId}` ||
      socket.search !== "" ||
      socket.hash !== "" ||
      target.webSocketDebuggerUrl !== expectedSocket
    )
      fail("cdp-target-invalid");
  } catch (error) {
    if (String(error?.message ?? error).startsWith("journey-")) throw error;
    fail("cdp-target-invalid");
  }
}

export function decodeBoundedScreenshot(encoded) {
  if (typeof encoded !== "string") fail("screenshot-invalid");
  if (encoded.length > MAX_SCREENSHOT_BASE64_CHARACTERS)
    fail("screenshot-oversize");
  if (encoded.length === 0 || encoded.length % 4 !== 0)
    fail("screenshot-invalid");
  const padding = encoded.endsWith("==") ? 2 : encoded.endsWith("=") ? 1 : 0;
  const alphabetLength = encoded.length - padding;
  for (let index = 0; index < alphabetLength; index += 1) {
    const code = encoded.charCodeAt(index);
    if (!(
      (code >= 65 && code <= 90) ||
      (code >= 97 && code <= 122) ||
      (code >= 48 && code <= 57) ||
      code === 43 ||
      code === 47
    ))
      fail("screenshot-invalid");
  }
  for (let index = alphabetLength; index < encoded.length; index += 1)
    if (encoded[index] !== "=") fail("screenshot-invalid");
  const bytes = Buffer.from(encoded, "base64");
  if (bytes.toString("base64") !== encoded) fail("screenshot-invalid");
  if (bytes.byteLength > MAX_ARTIFACT_BYTES) fail("screenshot-oversize");
  return bytes;
}

function parsePlaywrightValue(
  output,
  maximumResultBytes = MAX_PLAYWRIGHT_RESULT_BYTES,
) {
  if (
    typeof output !== "string" ||
    Buffer.byteLength(output, "utf8") >
      maximumResultBytes + MAX_PLAYWRIGHT_ENVELOPE_OVERHEAD_BYTES
  )
    fail("playwright-output-invalid");
  const headerLines = output
    .split("\n")
    .map((line) => (line.endsWith("\r") ? line.slice(0, -1) : line));
  const resultHeaderLines = headerLines.flatMap((line, index) =>
    line === "### Result" ? [index] : [],
  );
  const codeHeaderLines = headerLines.flatMap((line, index) =>
    line === "### Ran Playwright code" ? [index] : [],
  );
  if (
    resultHeaderLines.length !== 1 ||
    codeHeaderLines.length !== 1 ||
    resultHeaderLines[0] >= codeHeaderLines[0]
  )
    fail("playwright-output-invalid");
  const resultStart = output.indexOf(PLAYWRIGHT_RESULT_MARKER);
  const codeStart = output.indexOf(PLAYWRIGHT_CODE_MARKER);
  if (
    resultStart < 0 ||
    codeStart < resultStart + PLAYWRIGHT_RESULT_MARKER.length ||
    output.indexOf(
      PLAYWRIGHT_RESULT_MARKER,
      resultStart + PLAYWRIGHT_RESULT_MARKER.length,
    ) !== -1 ||
    output.indexOf(
      PLAYWRIGHT_CODE_MARKER,
      codeStart + PLAYWRIGHT_CODE_MARKER.length,
    ) !== -1
  )
    fail("playwright-output-invalid");
  const payload = output.slice(
    resultStart + PLAYWRIGHT_RESULT_MARKER.length,
    codeStart,
  );
  const ranCode = output.slice(codeStart + PLAYWRIGHT_CODE_MARKER.length);
  if (
    payload.length === 0 ||
    ranCode.trim().length === 0 ||
    Buffer.byteLength(payload, "utf8") > maximumResultBytes ||
    ranCode.includes(PLAYWRIGHT_RESULT_MARKER) ||
    ranCode.includes(PLAYWRIGHT_CODE_MARKER)
  )
    fail("playwright-output-invalid");
  let value;
  try {
    value = JSON.parse(payload);
  } catch {
    fail("playwright-output-invalid");
  }
  if (
    typeof value !== "string" &&
    typeof value !== "boolean" &&
    (!value || typeof value !== "object" || Array.isArray(value))
  )
    fail("playwright-output-invalid");
  return value;
}

export async function attachPlaywright(options, deadline, deps = {}) {
  const runTool = deps.runTool ?? runBoundedTool;
  const transport =
    deps.transport ?? resolvePlaywrightTransport(options.playwrightCli);
  const invoke = (args, callDeadline, toolDeps = {}) =>
    runPlaywrightTool(transport, args, callDeadline, options.output, {
      runTool,
      validateTransport: deps.validateTransport,
      ...toolDeps,
    });
  const session = `sage-ide-${process.pid}-${Date.now()}`;
  const base = ["--session", session];
  await invoke(
    [...base, "attach", "--cdp", `http://127.0.0.1:${options.port}`],
    deadline,
  );
  return {
    transport,
    evaluate: async (expression) =>
      parsePlaywrightValue(
        await invoke([...base, "eval", expression], deadline),
      ),
    inspectTarget: async () =>
      parsePlaywrightValue(
        await invoke(
          [
            ...base,
            "run-code",
            'async page => { const session = await page.context().newCDPSession(page); try { const { targetInfo } = await session.send("Target.getTargetInfo"); return { id: targetInfo.targetId, type: targetInfo.type, title: targetInfo.title, url: targetInfo.url }; } finally { await session.detach(); } }',
          ],
          deadline,
        ),
      ),
    screenshot: async (name) => {
      const encoded = parsePlaywrightValue(
        await invoke(
          [
            ...base,
            "run-code",
            "async page => (await page.screenshot()).toString('base64')",
          ],
          deadline,
          { maxOutputBytes: MAX_SCREENSHOT_OUTPUT_BYTES },
        ),
        MAX_SCREENSHOT_RESULT_BYTES,
      );
      const bytes = decodeBoundedScreenshot(encoded);
      const target = path.join(options.output, name);
      writeContainedBytes(options.sourceRoot, options.output, name, bytes);
      return target;
    },
    close: async () => {
      await invoke([...base, "close"], Date.now() + 5000);
    },
  };
}

async function revalidateAttachedPage(options, cdp, admittedTarget, deadline) {
  const currentTargets = await readCdpTargets(options.port, deadline);
  const controlledTarget = await cdp.inspectTarget();
  return revalidateControlledCdpTarget(
    admittedTarget,
    currentTargets,
    controlledTarget,
    options.port,
    options.expectedOrigin,
  );
}

function action(label, expression, assertion) {
  return { label, expression, assertion };
}

export const UI_ACTIONS = Object.freeze([
  action(
    "Project",
    '(()=>{const tools=[...document.querySelectorAll(\'aside[aria-label="Project tools"]\')];if(tools.length!==1)return false;const trees=[...tools[0].querySelectorAll(\'[role="tree"][aria-label="Project files"]\')];return trees.length===1&&trees[0].querySelectorAll(\'[role="treeitem"][aria-label="Open file journey.ts"]\').length===1})()',
    "Project root is not visible",
  ),
  action(
    "Explorer",
    "(()=>{const x=[...document.querySelectorAll('button[aria-label=\"Explorer\"]')];if(x.length!==1)return false;if(x[0].getAttribute('aria-expanded')==='true')return true;x[0].click();return true})()",
    "Explorer control is ambiguous",
  ),
  action(
    "Search",
    "(()=>{const x=[...document.querySelectorAll('button[aria-label=\"Search\"]')];if(x.length!==1)return false;x[0].click();return true})()",
    "Search control is ambiguous",
  ),
  action(
    "File",
    "(()=>{const forms=[...document.querySelectorAll('.workspace-search__form')];if(forms.length!==1)return false;const inputs=[...forms[0].querySelectorAll('input[type=\"search\"][aria-label=\"Search files\"]')];const buttons=[...forms[0].querySelectorAll('button[type=\"submit\"]')];if(inputs.length!==1||buttons.length!==1)return false;const set=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;set.call(inputs[0],'SAGE_JOURNEY_SENTINEL');inputs[0].dispatchEvent(new Event('input',{bubbles:true}));buttons[0].click();return true})()",
    "Search form is unavailable",
  ),
  action(
    "edit-diagnostic",
    '(()=>{const x=[...document.querySelectorAll(\'.workspace-search__results[role="list"] [role="listitem"] > button.workspace-search__result[aria-label="Open journey.ts at line 1, column 14"]\')];if(x.length!==1)return false;x[0].click();return true})()',
    "Search result was not unique",
  ),
  action(
    "save",
    "(()=>{const editors=[...document.querySelectorAll('section[aria-label=\"File editor journey.ts\"]')];if(editors.length!==1)return false;const buttons=[...editors[0].querySelectorAll('button[type=\"button\"]')].filter(x=>x.textContent.trim()==='Save');if(buttons.length!==1)return false;buttons[0].click();return true})()",
    "Save control is unavailable",
  ),
  action(
    "Source Control",
    "(()=>{const x=[...document.querySelectorAll('button[aria-label=\"Source Control\"]')];if(x.length!==1)return false;x[0].click();return true})()",
    "Source Control control is ambiguous",
  ),
  action(
    "Diff",
    "(()=>{const panels=[...document.querySelectorAll('section[aria-label=\"Source Control project tools\"]')];if(panels.length!==1)return false;const rows=[...panels[0].querySelectorAll('button[aria-label=\"Open diff for journey.ts\"]')];if(rows.length!==1)return false;rows[0].click();return true})()",
    "Diff control is unavailable",
  ),
]);

async function waitFor(evaluate, expression, deadline, message) {
  while (Date.now() < deadline) {
    if (await evaluate(expression)) return;
    await sleep(100);
  }
  throw new Error(message);
}

export function verificationFor(step) {
  if (step === "Explorer")
    return '(()=>{const tools=[...document.querySelectorAll(\'aside[aria-label="Project tools"]\')];if(tools.length!==1)return false;const trees=[...tools[0].querySelectorAll(\'[role="tree"][aria-label="Project files"]\')];return trees.length===1&&trees[0].querySelectorAll(\'[role="treeitem"][aria-label="Open file journey.ts"]\').length===1})()';
  if (step === "Search")
    return "(()=>{const forms=[...document.querySelectorAll('.workspace-search__form')];if(forms.length!==1)return false;return forms[0].querySelectorAll('input[type=\"search\"][aria-label=\"Search files\"]').length===1&&forms[0].querySelectorAll('button[type=\"submit\"]').length===1})()";
  if (step === "File")
    return 'document.querySelectorAll(\'.workspace-search__results[role="list"] [role="listitem"] > button.workspace-search__result[aria-label="Open journey.ts at line 1, column 14"]\').length===1';
  if (step === "edit-diagnostic")
    return 'document.querySelectorAll(\'section[aria-label="File editor journey.ts"] .cm-content[contenteditable="true"]\').length===1';
  if (step === "save")
    return "document.querySelectorAll('section[aria-label=\"File editor journey.ts\"]').length===1";
  if (step === "Source Control")
    return "(()=>{const panels=[...document.querySelectorAll('section[aria-label=\"Source Control project tools\"]')];return panels.length===1&&panels[0].querySelectorAll('button[aria-label=\"Open diff for journey.ts\"]').length===1})()";
  if (step === "Diff")
    return "(()=>{const surfaces=[...document.querySelectorAll('section.diff-surface[aria-label=\"Diff for journey.ts\"]')];if(surfaces.length!==1)return false;const ready=[...surfaces[0].querySelectorAll('.diff-editor[role=\"table\"][aria-label=\"Unified file diff\"]')];return ready.length===1&&ready[0].textContent.includes('journey edited through Playwright')})()";
  return UI_ACTIONS[0].expression;
}

export const UI_PROBES = Object.freeze({
  editableEditor:
    'document.querySelectorAll(\'section[aria-label="File editor journey.ts"] .cm-content[contenteditable="true"]\').length===1',
  insertEditorEdit:
    '(()=>{const x=[...document.querySelectorAll(\'section[aria-label="File editor journey.ts"] .cm-content[contenteditable="true"]\')];if(x.length!==1)return false;x[0].focus();return document.execCommand("insertText",false,"\\n// journey edited through Playwright\\n")})()',
  openProblems:
    "(()=>{const x=[...document.querySelectorAll('button[aria-label=\"Problems\"]')];if(x.length!==1)return false;x[0].click();return true})()",
  problemsReady:
    "(()=>{const panels=[...document.querySelectorAll('section[aria-label=\"Problems project tools\"]')];if(panels.length!==1)return false;const surfaces=[...panels[0].querySelectorAll('section[aria-label=\"Problems\"]')];return surfaces.length===1&&surfaces[0].querySelectorAll('button.problem-card').length>=1})()",
  diagnosticMessages:
    "(()=>{const panels=[...document.querySelectorAll('section[aria-label=\"Problems project tools\"]')];if(panels.length!==1)return null;const surfaces=[...panels[0].querySelectorAll('section[aria-label=\"Problems\"]')];if(surfaces.length!==1)return null;return JSON.stringify([...surfaces[0].querySelectorAll('button.problem-card')].map(x=>x.innerText))})()",
  saveSettled:
    "(()=>{const editors=[...document.querySelectorAll('section[aria-label=\"File editor journey.ts\"]')];if(editors.length!==1)return false;const buttons=[...editors[0].querySelectorAll('button[type=\"button\"]')].filter(x=>x.textContent.trim()==='Save');return buttons.length===1&&buttons[0].disabled===true})()",
  savedContent:
    "(()=>{const editors=[...document.querySelectorAll('section[aria-label=\"File editor journey.ts\"]')];if(editors.length!==1)return null;const contents=[...editors[0].querySelectorAll('.cm-content')];return contents.length===1?contents[0].innerText:null})()",
  diffContent:
    "(()=>{const surfaces=[...document.querySelectorAll('section.diff-surface[aria-label=\"Diff for journey.ts\"]')];return surfaces.length===1?surfaces[0].innerText:null})()",
});

export function assertEvidenceRoot(sourceRoot, output) {
  const evidenceParent = path.join(realpathSync(sourceRoot), "local-evidence");
  if (!existsSync(output) || !isWithin(evidenceParent, output))
    fail("evidence-root-invalid");
  let current = path.resolve(output);
  while (true) {
    if (lstatSync(current).isSymbolicLink()) fail("evidence-root-invalid");
    if (current === evidenceParent) break;
    const parent = path.dirname(current);
    if (parent === current) fail("evidence-root-invalid");
    current = parent;
  }
}

function writeContainedText(sourceRoot, output, name, value) {
  assertEvidenceRoot(sourceRoot, output);
  const target = path.join(output, name);
  const relative = path.relative(realpathSync(output), path.resolve(target));
  if (
    !relative ||
    relative.startsWith(`..${path.sep}`) ||
    path.isAbsolute(relative) ||
    existsSync(target) ||
    lstatSync(output).isSymbolicLink()
  )
    fail("evidence-root-invalid");
  writeFileSync(target, value, { flag: "wx" });
}

export function writePendingJourneyReceipt(
  sourceRoot,
  output,
  receiptName,
  receipt,
) {
  if (!PENDING_RECEIPT_NAME.test(receiptName)) fail("receipt-name-invalid");
  writeContainedText(
    sourceRoot,
    output,
    receiptName,
    `${JSON.stringify(receipt, null, 2)}\n`,
  );
  return path.join(output, receiptName);
}

function writeContainedBytes(sourceRoot, output, name, value) {
  assertEvidenceRoot(sourceRoot, output);
  if (!Buffer.isBuffer(value) || value.byteLength > MAX_ARTIFACT_BYTES)
    fail("screenshot-oversize");
  const target = path.join(output, name);
  const relative = path.relative(realpathSync(output), path.resolve(target));
  if (!relative || relative.includes(path.sep) || existsSync(target))
    fail("evidence-root-invalid");
  writeFileSync(target, value, { flag: "wx" });
}

async function execute(options) {
  assertEvidenceRoot(options.sourceRoot, options.output);
  const executableBinding = {
    bytes: options.executableBytes,
    sha256: options.executableSha256,
  };
  validateExecutableBinding(options.executable, executableBinding);
  const deadline = Date.now() + options.deadlineMs;
  const admittedTarget = await discoverTarget(
    options.port,
    options.expectedOrigin,
    deadline,
  );
  const cdp = await attachPlaywright(options, deadline);
  const artifacts = [];
  const milestones = [];
  const log = [
    {
      ordinal: 0,
      step: "frontend-ready",
      runtimeMode: options.runtimeMode,
      ready: options.frontendReady,
    },
  ];
  let diagnosticMessages = null;
  let savedContent = null;
  let diffContent = null;
  let controlledTarget;
  try {
    await revalidateAttachedPage(options, cdp, admittedTarget, deadline);
    for (let index = 0; index < UI_ACTIONS.length; index += 1) {
      const current = UI_ACTIONS[index];
      if ((await cdp.evaluate(current.expression)) !== true)
        throw new Error(current.assertion);
      await waitFor(
        cdp.evaluate,
        verificationFor(current.label),
        deadline,
        `${current.label} verification timed out`,
      );
      if (current.label === "edit-diagnostic") {
        if (!(await cdp.evaluate(UI_PROBES.editableEditor)))
          throw new Error(
            "editor did not expose one editable CodeMirror surface",
          );
        if ((await cdp.evaluate(UI_PROBES.insertEditorEdit)) !== true)
          throw new Error("editor did not accept native text insertion");
        if ((await cdp.evaluate(UI_PROBES.openProblems)) !== true)
          throw new Error("Problems control is ambiguous");
        await waitFor(
          cdp.evaluate,
          UI_PROBES.problemsReady,
          deadline,
          "language diagnostic did not arrive",
        );
        diagnosticMessages = await cdp.evaluate(UI_PROBES.diagnosticMessages);
      }
      if (current.label === "save") {
        await waitFor(
          cdp.evaluate,
          UI_PROBES.saveSettled,
          deadline,
          "save did not settle to a disabled control",
        );
        savedContent = await cdp.evaluate(UI_PROBES.savedContent);
      }
      if (current.label === "Diff") {
        diffContent = await cdp.evaluate(UI_PROBES.diffContent);
      }
      controlledTarget = await revalidateAttachedPage(
        options,
        cdp,
        admittedTarget,
        deadline,
      );
      const screenshotName = `${String(index + 1).padStart(2, "0")}-${current.label.replaceAll(/[^a-z0-9]+/giu, "-").toLowerCase()}.png`;
      const artifact = await cdp.screenshot(screenshotName);
      artifacts.push(artifact);
      milestones.push({ step: current.label, ordinal: index + 1 });
      log.push({
        ordinal: index + 1,
        step: current.label,
        captured: path.basename(artifact),
      });
    }
    const logPath = path.join(options.output, "cdp-milestones.json");
    writeContainedText(
      options.sourceRoot,
      options.output,
      "cdp-milestones.json",
      `${JSON.stringify(log, null, 2)}\n`,
    );
    artifacts.push(logPath);
    const diagnostics = JSON.parse(String(diagnosticMessages));
    const disk = readFileSync(path.join(options.fixture, "journey.ts"), "utf8");
    if (
      savedContent !== disk ||
      !disk.includes("journey edited through Playwright") ||
      !Array.isArray(diagnostics) ||
      diagnostics.length < 1 ||
      typeof diffContent !== "string" ||
      !diffContent.includes("journey.ts") ||
      !diffContent.includes("journey edited through Playwright")
    )
      fail("observations-invalid");
    const digest = (value) => createHash("sha256").update(value).digest("hex");
    const observations = {
      save: {
        editorPath: "journey.ts",
        saveDisabled: true,
        contentSha256: digest(savedContent),
        diskSha256: digest(disk),
      },
      diagnostic: {
        resource: "journey.ts",
        count: diagnostics.length,
        messagesSha256: digest(JSON.stringify(diagnostics)),
      },
      diff: {
        resource: "journey.ts",
        lineCount: diffContent.split("\n").length,
        contentSha256: digest(diffContent),
      },
    };
    validatePlaywrightTransport(cdp.transport);
    const receipt = createJourneyReceipt({
      source: { commit: options.sourceCommit, tree: options.sourceTree },
      executable: {
        path: options.executable,
        ...validateExecutableBinding(options.executable, executableBinding),
      },
      playwrightTransport: createPlaywrightTransportReceipt(cdp.transport),
      fixture: { path: options.fixture, sha256: hashFixture(options.fixture) },
      cdp: {
        port: options.port,
        targetId: controlledTarget.id,
        origin: options.expectedOrigin,
      },
      expectedOrigin: options.expectedOrigin,
      runtime: {
        mode: options.runtimeMode,
        frontendReady: options.frontendReady,
      },
      milestones,
      artifacts,
      evidenceRoot: options.output,
      observations,
    });
    writePendingJourneyReceipt(
      options.sourceRoot,
      options.output,
      options.receiptName,
      receipt,
    );
  } finally {
    await cdp.close();
  }
}

function hashFixture(root) {
  const files = ["journey.ts", "fixture-identity.json"].map((relative) =>
    path.join(root, relative),
  );
  if (!files.every((file) => existsSync(file))) fail("fixture-invalid");
  return createHash("sha256")
    .update(
      files
        .map((file) => `${path.basename(file)}\0${hashFile(file).sha256}`)
        .join("\n"),
    )
    .digest("hex");
}

if (
  process.argv[1] &&
  path.resolve(fileURLToPath(import.meta.url)) === path.resolve(process.argv[1])
) {
  let options;
  try {
    options = parseJourneyArguments(process.argv.slice(2));
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
      /* never write a receipt through an unsafe root */
    }
    console.error(String(error?.message ?? error).slice(0, 1024));
    process.exitCode = 1;
  }
}
