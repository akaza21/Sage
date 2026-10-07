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
import { connectCdp } from "./source-functional-native-ide-cdp.mjs";
import {
  resolvePlaywrightTransport,
  runPlaywrightTool,
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
  "error-recovery",
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
const MAX_CDP_OPERATION_MS = 10_000;
const MIN_CDP_JOURNEY_REQUESTS = 41;
const MAX_CDP_JOURNEY_REQUESTS = 1024;
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
  if (value === "tauri://localhost/" || value === "http://tauri.localhost/")
    return value;
  const match = LOOPBACK_HTTP_ORIGIN.exec(value);
  if (!match) return null;
  const port = Number(match.groups.port);
  if (!Number.isInteger(port) || port < 1024 || port > 65535) return null;
  return value.endsWith("/") ? value : `${value}/`;
}

export function hashFile(filePath, maximumBytes = MAX_ARTIFACT_BYTES) {
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

function sameCanonicalPath(left, right) {
  const normalize = (value) => {
    let canonical = value;
    if (process.platform === "win32") {
      const lower = canonical.toLowerCase();
      if (lower.startsWith("\\\\?\\unc\\"))
        canonical = `\\\\${canonical.slice(8)}`;
      else if (lower.startsWith("\\\\?\\")) canonical = canonical.slice(4);
      canonical = canonical.toLowerCase();
    }
    return path.normalize(canonical);
  };
  return normalize(left) === normalize(right);
}

export function validatePlainCanonicalExecutable(
  filePath,
  expected,
  deps = {},
) {
  if (
    typeof filePath !== "string" ||
    !path.isAbsolute(filePath) ||
    path.resolve(filePath) !== filePath
  )
    fail("executable-identity-invalid");
  const inspect = deps.lstatSync ?? lstatSync;
  const resolveRealPath =
    deps.realpathSync ?? realpathSync.native ?? realpathSync;
  let current = filePath;
  while (true) {
    const stats = inspect(current);
    if (stats.isSymbolicLink()) fail("executable-identity-invalid");
    if (!sameCanonicalPath(resolveRealPath(current), current))
      fail("executable-identity-invalid");
    const parent = path.dirname(current);
    if (parent === current) break;
    current = parent;
  }
  const validate = deps.validateBinding ?? validateExecutableBinding;
  return Object.freeze({ path: filePath, ...validate(filePath, expected) });
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
    "executable-bytes",
    "executable-sha256",
    "node-bytes",
    "node-sha256",
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
  const nodeBytes = Number(options["node-bytes"]);
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
    !Number.isSafeInteger(executableBytes) ||
    executableBytes <= 0 ||
    executableBytes > MAX_EXECUTABLE_BYTES ||
    !SHA256.test(options["executable-sha256"] ?? "") ||
    !Number.isSafeInteger(nodeBytes) ||
    nodeBytes <= 0 ||
    nodeBytes > MAX_EXECUTABLE_BYTES ||
    !SHA256.test(options["node-sha256"] ?? "") ||
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
    executableBytes,
    executableSha256: options["executable-sha256"],
    nodeBytes,
    nodeSha256: options["node-sha256"],
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
  automationTransport,
  fixture,
  cdp,
  expectedOrigin,
  runtime,
  milestones,
  artifacts,
  observations,
  evidenceRoot,
}) {
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
    runtime.frontendReady !== true ||
    !exactKeys(automationTransport, [
      "kind",
      "node",
      "connectionCount",
      "maxInFlight",
      "requestCount",
    ]) ||
    automationTransport.kind !== "node-websocket-cdp-v1" ||
    !exactKeys(automationTransport.node, ["path", "bytes", "sha256"]) ||
    !isAbsolutePath(automationTransport.node.path) ||
    (path.isAbsolute(automationTransport.node.path)
      ? path.resolve(automationTransport.node.path) !==
        automationTransport.node.path
      : path.win32.normalize(automationTransport.node.path) !==
        automationTransport.node.path) ||
    !Number.isSafeInteger(automationTransport.node.bytes) ||
    automationTransport.node.bytes <= 0 ||
    automationTransport.node.bytes > MAX_EXECUTABLE_BYTES ||
    !SHA256.test(automationTransport.node.sha256) ||
    automationTransport.connectionCount !== 1 ||
    automationTransport.maxInFlight !== 1 ||
    !Number.isSafeInteger(automationTransport.requestCount) ||
    automationTransport.requestCount < MIN_CDP_JOURNEY_REQUESTS ||
    automationTransport.requestCount > MAX_CDP_JOURNEY_REQUESTS
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
    !exactKeys(observations, ["diagnostic", "diff", "recovery", "save"]) ||
    !exactKeys(observations.save, [
      "contentSha256",
      "dirtyIndicatorCleared",
      "diskSha256",
      "editorPath",
    ]) ||
    observations.save.editorPath !== "journey.ts" ||
    observations.save.dirtyIndicatorCleared !== true ||
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
    !SHA256.test(observations.diff.contentSha256) ||
    !exactKeys(observations.recovery, [
      "conflictPresented",
      "diskSha256",
      "recovered",
      "resource",
    ]) ||
    observations.recovery.resource !== "journey.ts" ||
    observations.recovery.conflictPresented !== true ||
    observations.recovery.recovered !== true ||
    !SHA256.test(observations.recovery.diskSha256)
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
    schemaVersion: 5,
    completed: true,
    source: Object.freeze({ ...source }),
    sourceBinding: "collection-only",
    runtime: Object.freeze({ ...runtime }),
    executable: Object.freeze({
      bytes: executable.bytes,
      sha256: executable.sha256,
    }),
    automationTransport: Object.freeze({
      kind: automationTransport.kind,
      node: Object.freeze({ ...automationTransport.node }),
      connectionCount: automationTransport.connectionCount,
      maxInFlight: automationTransport.maxInFlight,
      requestCount: automationTransport.requestCount,
    }),
    fixture: Object.freeze({ sha256: fixture.sha256 }),
    cdp: Object.freeze({ ...cdp }),
    observations: Object.freeze({
      save: Object.freeze({ ...observations.save }),
      diagnostic: Object.freeze({ ...observations.diagnostic }),
      diff: Object.freeze({ ...observations.diff }),
      recovery: Object.freeze({ ...observations.recovery }),
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
  return Object.freeze({
    id: pages[0].id,
    type: "page",
    title: pages[0].title,
    url: pages[0].url,
    webSocketDebuggerUrl: pages[0].webSocketDebuggerUrl,
  });
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

export async function readCdpTargets(port, deadline) {
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

export async function discoverTarget(port, expectedOrigin, deadline) {
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

export async function revalidateAttachedPage(
  options,
  cdp,
  admittedTarget,
  deadline,
) {
  const currentTargets = await readCdpTargets(options.port, deadline);
  const pageIdentity = await cdp.evaluate(
    "(()=>({title:document.title,url:location.href}))()",
    operationDeadline(deadline),
  );
  const controlledTarget = {
    id: admittedTarget.id,
    type: "page",
    title: pageIdentity?.title,
    url: pageIdentity?.url,
  };
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
    "(()=>{const forms=[...document.querySelectorAll('.workspace-search__form')];if(forms.length!==1)return false;const inputs=[...forms[0].querySelectorAll('input[type=\"search\"][aria-label=\"Search text\"]')];if(inputs.length!==1)return false;const set=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;set.call(inputs[0],'SAGE_JOURNEY_SENTINEL');inputs[0].dispatchEvent(new Event('input',{bubbles:true}));return forms[0].dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}))===false})()",
    "Search form is unavailable",
  ),
  action(
    "edit-diagnostic",
    "(()=>{const x=[...document.querySelectorAll('button[aria-label=\"Problems\"]')];if(x.length!==1)return false;x[0].click();return true})()",
    "Problems control is ambiguous",
  ),
  action(
    "save",
    "(()=>{const editors=[...document.querySelectorAll('section[aria-label=\"File editor journey.ts\"]')];if(editors.length!==1)return false;const event=new KeyboardEvent('keydown',{key:'s',code:'KeyS',ctrlKey:true,bubbles:true,cancelable:true});editors[0].dispatchEvent(event);return event.defaultPrevented})()",
    "Save shortcut was not handled",
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
  action(
    "error-recovery",
    "(()=>{const x=[...document.querySelectorAll('button[aria-label=\"Explorer\"]')];if(x.length!==1)return false;x[0].click();return true})()",
    "Explorer control is unavailable for recovery",
  ),
]);

export function operationDeadline(journeyDeadline) {
  const now = Date.now();
  if (now >= journeyDeadline) throw new Error("journey deadline exhausted");
  return Math.min(journeyDeadline, now + MAX_CDP_OPERATION_MS);
}

export async function waitFor(cdp, expression, deadline, message) {
  while (Date.now() < deadline) {
    if (await cdp.evaluate(expression, operationDeadline(deadline))) return;
    await sleep(Math.min(100, Math.max(0, deadline - Date.now())));
  }
  throw new Error(message);
}

export async function performUiAction(cdp, current, deadline) {
  if (current.label === "Project") {
    await waitFor(
      cdp,
      current.expression,
      operationDeadline(deadline),
      "Project readiness timed out",
    );
    return;
  }
  if (
    (await cdp.evaluate(current.expression, operationDeadline(deadline))) !==
    true
  )
    throw new Error(current.assertion);
  await waitFor(
    cdp,
    verificationFor(current.label),
    deadline,
    `${current.label} verification timed out`,
  );
}

export function attachCdp(admittedTarget, deadline, deps = {}) {
  const connect = deps.connectCdp ?? connectCdp;
  return connect({
    target: admittedTarget,
    deadline,
    openDeadline: operationDeadline(deadline),
    ...(deps.WebSocketImpl ? { WebSocketImpl: deps.WebSocketImpl } : {}),
  });
}

export function verificationFor(step) {
  if (step === "Explorer")
    return '(()=>{const tools=[...document.querySelectorAll(\'aside[aria-label="Project tools"]\')];if(tools.length!==1)return false;const trees=[...tools[0].querySelectorAll(\'[role="tree"][aria-label="Project files"]\')];return trees.length===1&&trees[0].querySelectorAll(\'[role="treeitem"][aria-label="Open file journey.ts"]\').length===1})()';
  if (step === "Search")
    return "(()=>{const forms=[...document.querySelectorAll('.workspace-search__form')];if(forms.length!==1)return false;return forms[0].querySelectorAll('input[type=\"search\"][aria-label=\"Search text\"]').length===1})()";
  if (step === "File")
    return 'document.querySelectorAll(\'.workspace-search__results[role="list"] [role="listitem"] > button.workspace-search__result[aria-label="Open journey.ts at line 1, column 14"]\').length===1';
  if (step === "edit-diagnostic")
    return "(()=>{const panels=[...document.querySelectorAll('section[aria-label=\"Problems project tools\"]')];if(panels.length!==1)return false;const surfaces=[...panels[0].querySelectorAll('section[aria-label=\"Problems\"]')];return surfaces.length===1&&surfaces[0].getAttribute('data-diagnostic-listener-ready')==='true'})()";
  if (step === "save")
    return "document.querySelectorAll('section[aria-label=\"File editor journey.ts\"]').length===1";
  if (step === "Source Control")
    return "(()=>{const panels=[...document.querySelectorAll('section[aria-label=\"Source Control project tools\"]')];return panels.length===1&&panels[0].querySelectorAll('button[aria-label=\"Open diff for journey.ts\"]').length===1})()";
  if (step === "Diff")
    return "(()=>{const surfaces=[...document.querySelectorAll('section.diff-surface[aria-label=\"Diff for journey.ts\"]')];if(surfaces.length!==1)return false;const ready=[...surfaces[0].querySelectorAll('.diff-editor[role=\"table\"][aria-label=\"Unified file diff\"]')];return ready.length===1&&ready[0].textContent.includes('// SAGE_TS_DIAGNOSTIC journey edited through native CDP')})()";
  if (step === "error-recovery")
    return '(()=>{const tools=[...document.querySelectorAll(\'aside[aria-label="Project tools"]\')];if(tools.length!==1)return false;const trees=[...tools[0].querySelectorAll(\'[role="tree"][aria-label="Project files"]\')];return trees.length===1&&trees[0].querySelectorAll(\'[role="treeitem"][aria-label="Open file journey.ts"]\').length===1})()';
  return UI_ACTIONS[0].expression;
}

export const UI_PROBES = Object.freeze({
  openExactSearchResult:
    '(()=>{const x=[...document.querySelectorAll(\'.workspace-search__results[role="list"] [role="listitem"] > button.workspace-search__result[aria-label="Open journey.ts at line 1, column 14"]\')];if(x.length!==1)return false;x[0].click();return true})()',
  editableEditor:
    'document.querySelectorAll(\'section[aria-label="File editor journey.ts"] .cm-content[contenteditable="true"]\').length===1',
  insertEditorEdit:
    '(()=>{const x=[...document.querySelectorAll(\'section[aria-label="File editor journey.ts"] .cm-content[contenteditable="true"]\')];if(x.length!==1)return false;x[0].focus();return document.execCommand("insertText",false,"\\n// SAGE_TS_DIAGNOSTIC journey edited through native CDP\\n")})()',
  openProblems:
    "(()=>{const x=[...document.querySelectorAll('button[aria-label=\"Problems\"]')];if(x.length!==1)return false;x[0].click();return true})()",
  problemsMounted:
    "(()=>{const panels=[...document.querySelectorAll('section[aria-label=\"Problems project tools\"]')];if(panels.length!==1)return false;const surfaces=[...panels[0].querySelectorAll('section[aria-label=\"Problems\"]')];return surfaces.length===1&&surfaces[0].getAttribute('data-diagnostic-listener-ready')==='true'})()",
  problemsReady:
    "(()=>{const panels=[...document.querySelectorAll('section[aria-label=\"Problems project tools\"]')];if(panels.length!==1)return false;const surfaces=[...panels[0].querySelectorAll('section[aria-label=\"Problems\"]')];return surfaces.length===1&&surfaces[0].querySelectorAll('button.problem-card').length>=1})()",
  diagnosticMessages:
    "(()=>{const panels=[...document.querySelectorAll('section[aria-label=\"Problems project tools\"]')];if(panels.length!==1)return null;const surfaces=[...panels[0].querySelectorAll('section[aria-label=\"Problems\"]')];if(surfaces.length!==1)return null;return JSON.stringify([...surfaces[0].querySelectorAll('button.problem-card')].map(x=>x.innerText))})()",
  saveSettled:
    "(()=>{const editors=[...document.querySelectorAll('section[aria-label=\"File editor journey.ts\"]')];if(editors.length!==1)return false;const unsaved=editors[0].querySelectorAll('.file-header__save-state--dirty').length;const conflicts=editors[0].querySelectorAll('.editor-notice--conflict').length;const dirtyTabs=document.querySelectorAll('.tab--active .tab__dirty').length;return unsaved===0&&conflicts===0&&dirtyTabs===0})()",
  savedContent:
    "(()=>{const editors=[...document.querySelectorAll('section[aria-label=\"File editor journey.ts\"]')];if(editors.length!==1)return null;const contents=[...editors[0].querySelectorAll('.cm-content')];return contents.length===1?contents[0].innerText:null})()",
  diffContent:
    "(()=>{const surfaces=[...document.querySelectorAll('section.diff-surface[aria-label=\"Diff for journey.ts\"]')];return surfaces.length===1?surfaces[0].innerText:null})()",
  openExactExplorerFile:
    '(()=>{const x=[...document.querySelectorAll(\'[role="tree"][aria-label="Project files"] [role="treeitem"][aria-label="Open file journey.ts"]\')];if(x.length!==1)return false;x[0].click();return true})()',
  insertConflictEdit:
    '(()=>{const x=[...document.querySelectorAll(\'section[aria-label="File editor journey.ts"] .cm-content[contenteditable="true"]\')];if(x.length!==1)return false;x[0].focus();return document.execCommand("insertText",false,"\\n// SAGE_CONFLICT_EDIT native recovery\\n")})()',
  conflictVisible:
    "(()=>{const alerts=[...document.querySelectorAll('section[aria-label=\"File editor journey.ts\"] .editor-notice--conflict[role=\"alert\"]')];return alerts.length===1&&alerts[0].innerText.includes('External change detected')})()",
  closeDirtyFile:
    "(()=>{const tabs=[...document.querySelectorAll('.tab')].filter(x=>x.querySelector('.file-kind-icon')&&x.textContent.includes('journey.ts'));if(tabs.length!==1)return false;const buttons=[...tabs[0].querySelectorAll('button[aria-label=\"Close journey.ts\"]')];if(buttons.length!==1)return false;buttons[0].click();return true})()",
  dirtyDialogVisible:
    "(()=>{const dialogs=[...document.querySelectorAll('section.dirty-dialog[role=\"dialog\"]')];return dialogs.length===1&&dialogs[0].innerText.includes('Save changes to journey.ts?')})()",
  discardDirtyClose:
    "(()=>{const dialogs=[...document.querySelectorAll('section.dirty-dialog[role=\"dialog\"]')];if(dialogs.length!==1)return false;const buttons=[...dialogs[0].querySelectorAll('button')].filter(x=>x.textContent.trim()==='Discard');if(buttons.length!==1)return false;buttons[0].click();return true})()",
  recoverySettled:
    '(()=>document.querySelectorAll(\'section.dirty-dialog[role="dialog"]\').length===0&&document.querySelectorAll(\'section[aria-label="File editor journey.ts"] .editor-notice--conflict[role="alert"]\').length===0)()',
});

export async function performUiMilestone(cdp, current, deadline) {
  await performUiAction(cdp, current, deadline);
  if (current.label === "File") {
    if (
      (await cdp.evaluate(
        UI_PROBES.openExactSearchResult,
        operationDeadline(deadline),
      )) !== true
    )
      throw new Error("Search result was not unique");
    await waitFor(
      cdp,
      UI_PROBES.editableEditor,
      deadline,
      "journey.ts editor did not become editable",
    );
  }
  if (current.label === "edit-diagnostic") {
    if (
      (await cdp.evaluate(
        UI_PROBES.insertEditorEdit,
        operationDeadline(deadline),
      )) !== true
    )
      throw new Error("editor did not accept native text insertion");
    await waitFor(
      cdp,
      UI_PROBES.problemsReady,
      deadline,
      "language diagnostic did not arrive",
    );
    return cdp.evaluate(
      UI_PROBES.diagnosticMessages,
      operationDeadline(deadline),
    );
  }
  if (current.label === "error-recovery") {
    if (
      (await cdp.evaluate(
        UI_PROBES.openExactExplorerFile,
        operationDeadline(deadline),
      )) !== true
    )
      throw new Error("recovery file was not unique");
    await waitFor(
      cdp,
      UI_PROBES.editableEditor,
      deadline,
      "recovery editor did not become editable",
    );
    if (
      (await cdp.evaluate(
        UI_PROBES.insertConflictEdit,
        operationDeadline(deadline),
      )) !== true
    )
      throw new Error("recovery edit was not accepted");
  }
  return null;
}

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

export function writeContainedText(sourceRoot, output, name, value) {
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

export function writeContainedBytes(sourceRoot, output, name, value) {
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
  const nodeBinding = {
    bytes: options.nodeBytes,
    sha256: options.nodeSha256,
  };
  validatePlainCanonicalExecutable(process.execPath, nodeBinding);
  const deadline = Date.now() + options.deadlineMs;
  const admittedTarget = await discoverTarget(
    options.port,
    options.expectedOrigin,
    deadline,
  );
  const cdp = await attachCdp(admittedTarget, deadline);
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
  let savedDisk = null;
  let diffContent = null;
  let recoveryDisk = null;
  let controlledTarget;
  let receipt;
  let journeyError;
  try {
    await revalidateAttachedPage(options, cdp, admittedTarget, deadline);
    for (let index = 0; index < UI_ACTIONS.length; index += 1) {
      const current = UI_ACTIONS[index];
      const milestoneObservation = await performUiMilestone(
        cdp,
        current,
        deadline,
      );
      if (current.label === "edit-diagnostic")
        diagnosticMessages = milestoneObservation;
      if (current.label === "save") {
        await waitFor(
          cdp,
          UI_PROBES.saveSettled,
          deadline,
          "save did not clear the dirty indicator",
        );
        savedContent = await cdp.evaluate(
          UI_PROBES.savedContent,
          operationDeadline(deadline),
        );
        savedDisk = readFileSync(
          path.join(options.fixture, "journey.ts"),
          "utf8",
        );
      }
      if (current.label === "Diff") {
        diffContent = await cdp.evaluate(
          UI_PROBES.diffContent,
          operationDeadline(deadline),
        );
      }
      if (current.label === "error-recovery") {
        const externalContent = `${savedDisk}\n// SAGE_EXTERNAL_CHANGE native recovery\n`;
        writeFileSync(
          path.join(options.fixture, "journey.ts"),
          externalContent,
          "utf8",
        );
        const saveAction = UI_ACTIONS.find((item) => item.label === "save");
        if (
          !saveAction ||
          (await cdp.evaluate(
            saveAction.expression,
            operationDeadline(deadline),
          )) !== true
        )
          throw new Error("recovery save shortcut was not handled");
        await waitFor(
          cdp,
          UI_PROBES.conflictVisible,
          deadline,
          "external-change conflict was not presented",
        );
        const errorScreenshotName = "09-error-presented.png";
        const errorScreenshot = await cdp.screenshot(
          operationDeadline(deadline),
        );
        const errorArtifact = path.join(options.output, errorScreenshotName);
        writeContainedBytes(
          options.sourceRoot,
          options.output,
          errorScreenshotName,
          decodeBoundedScreenshot(errorScreenshot),
        );
        artifacts.push(errorArtifact);
        log.push({
          ordinal: index + 1,
          step: "error-presented",
          captured: errorScreenshotName,
        });
        if (
          (await cdp.evaluate(
            UI_PROBES.closeDirtyFile,
            operationDeadline(deadline),
          )) !== true
        )
          throw new Error("dirty recovery file did not close");
        await waitFor(
          cdp,
          UI_PROBES.dirtyDialogVisible,
          deadline,
          "dirty close decision did not appear",
        );
        if (
          (await cdp.evaluate(
            UI_PROBES.discardDirtyClose,
            operationDeadline(deadline),
          )) !== true
        )
          throw new Error("dirty close discard was unavailable");
        await waitFor(
          cdp,
          UI_PROBES.recoverySettled,
          deadline,
          "external-change recovery did not settle",
        );
        recoveryDisk = readFileSync(
          path.join(options.fixture, "journey.ts"),
          "utf8",
        );
      }
      controlledTarget = await revalidateAttachedPage(
        options,
        cdp,
        admittedTarget,
        deadline,
      );
      const screenshotName = `${String(index + 1).padStart(2, "0")}-${current.label.replaceAll(/[^a-z0-9]+/giu, "-").toLowerCase()}.png`;
      const screenshot = await cdp.screenshot(operationDeadline(deadline));
      const artifact = path.join(options.output, screenshotName);
      writeContainedBytes(
        options.sourceRoot,
        options.output,
        screenshotName,
        decodeBoundedScreenshot(screenshot),
      );
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
    if (
      savedContent !== savedDisk ||
      !savedDisk.includes(
        "// SAGE_TS_DIAGNOSTIC journey edited through native CDP",
      ) ||
      !Array.isArray(diagnostics) ||
      diagnostics.length < 1 ||
      !diagnostics.some(
        (message) =>
          typeof message === "string" &&
          message.includes("TypeScript fixture diagnostic"),
      ) ||
      typeof diffContent !== "string" ||
      !diffContent.includes("journey.ts") ||
      !diffContent.includes(
        "// SAGE_TS_DIAGNOSTIC journey edited through native CDP",
      ) ||
      typeof recoveryDisk !== "string" ||
      !recoveryDisk.includes("// SAGE_EXTERNAL_CHANGE native recovery") ||
      recoveryDisk.includes("// SAGE_CONFLICT_EDIT native recovery")
    )
      fail("observations-invalid");
    const digest = (value) => createHash("sha256").update(value).digest("hex");
    const observations = {
      save: {
        editorPath: "journey.ts",
        dirtyIndicatorCleared: true,
        contentSha256: digest(savedContent),
        diskSha256: digest(savedDisk),
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
      recovery: {
        resource: "journey.ts",
        conflictPresented: true,
        recovered: true,
        diskSha256: digest(recoveryDisk),
      },
    };
    const finalNodeIdentity = validatePlainCanonicalExecutable(
      process.execPath,
      nodeBinding,
    );
    if (
      cdp.stats?.connectionCount !== 1 ||
      cdp.stats?.maxInFlight !== 1 ||
      !Number.isSafeInteger(cdp.stats?.requestCount) ||
      cdp.stats.requestCount < MIN_CDP_JOURNEY_REQUESTS ||
      cdp.stats.requestCount > MAX_CDP_JOURNEY_REQUESTS
    )
      fail("cdp-transport-invalid");
    receipt = createJourneyReceipt({
      source: { commit: options.sourceCommit, tree: options.sourceTree },
      executable: {
        path: options.executable,
        ...validateExecutableBinding(options.executable, executableBinding),
      },
      automationTransport: {
        kind: "node-websocket-cdp-v1",
        node: finalNodeIdentity,
        connectionCount: cdp.stats.connectionCount,
        maxInFlight: cdp.stats.maxInFlight,
        requestCount: cdp.stats.requestCount,
      },
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
  } catch (error) {
    journeyError = error;
  }
  let cleanupError;
  try {
    await cdp.close(Date.now() + 5000);
  } catch (error) {
    cleanupError = error;
  }
  if (journeyError) {
    if (cleanupError) {
      Object.defineProperty(journeyError, "cleanupError", {
        value: String(cleanupError?.message ?? cleanupError).slice(0, 1024),
        enumerable: true,
      });
    }
    throw journeyError;
  }
  if (cleanupError) throw cleanupError;
  writePendingJourneyReceipt(
    options.sourceRoot,
    options.output,
    options.receiptName,
    receipt,
  );
  return receipt;
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
          `${JSON.stringify(
            {
              schemaVersion: 1,
              completed: false,
              error: String(error?.message ?? error).slice(0, 1024),
              ...(typeof error?.cleanupError === "string"
                ? { cleanupError: error.cleanupError }
                : {}),
            },
            null,
            2,
          )}\n`,
        );
    } catch {
      /* never write a receipt through an unsafe root */
    }
    console.error(String(error?.message ?? error).slice(0, 1024));
    process.exitCode = 1;
  }
}
