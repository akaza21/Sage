import { createHash } from "node:crypto";
import { spawn, spawnSync } from "node:child_process";
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
    schemaVersion: 1,
    completed: true,
    source: Object.freeze({ ...source }),
    sourceBinding: "collection-only",
    runtime: Object.freeze({ ...runtime }),
    executable: Object.freeze({
      bytes: executable.bytes,
      sha256: executable.sha256,
    }),
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
  if (
    typeof encoded !== "string" ||
    Buffer.byteLength(encoded, "ascii") >
      Math.ceil((MAX_ARTIFACT_BYTES * 4) / 3)
  )
    fail("screenshot-oversize");
  const bytes = Buffer.from(encoded, "base64");
  if (bytes.byteLength > MAX_ARTIFACT_BYTES) fail("screenshot-oversize");
  return bytes;
}

export function processIdentityHelper(action, value) {
  const helper = fileURLToPath(
    new URL("./run-source-functional-native-ide-journey.ps1", import.meta.url),
  );
  const powershell = path.join(
    process.env.SystemRoot ?? "C:\\Windows",
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const args = [
    "-NoProfile",
    "-NonInteractive",
    "-File",
    helper,
    "-ToolIdentityAction",
    action,
  ];
  if (action === "capture") args.push("-ToolProcessId", String(value));
  else
    args.push(
      "-ToolIdentityJson",
      Buffer.from(JSON.stringify(value), "utf8").toString("base64"),
    );
  const result = spawnSync(powershell, args, {
    windowsHide: true,
    encoding: "utf8",
    timeout: 5000,
    maxBuffer: 64 * 1024,
  });
  if (result.error || result.status !== 0) fail("tool-cleanup-unproven");
  try {
    return JSON.parse(result.stdout.trim());
  } catch {
    fail("tool-cleanup-unproven");
  }
}

export function runBoundedTool(executable, args, deadline, cwd, deps = {}) {
  const spawnTool = deps.spawn ?? spawn;
  const maximumOutputBytes = deps.maxOutputBytes ?? 64 * 1024;
  const remaining = deadline - Date.now();
  if (remaining <= 0) fail("deadline-exhausted");
  return new Promise((resolve, reject) => {
    const child = spawnTool(executable, args, {
      cwd,
      windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
    });
    const chunks = [];
    let bytes = 0;
    let terminal = null;
    let identity;
    let settled = false;
    let settlementTimer = null;
    const quarantine = (category, cleanup = null) => {
      const record = {
        schemaVersion: 1,
        observedPid: child.pid ?? null,
        commandRole: String(deps.commandRole ?? "playwright-helper").slice(
          0,
          64,
        ),
        category,
        capturedAtUtc: new Date().toISOString(),
        deadlineUtc: new Date(deadline).toISOString(),
        cleanup: cleanup
          ? {
              identityLost: Boolean(cleanup.identityLost),
              remainingCount: Array.isArray(cleanup.remaining)
                ? cleanup.remaining.length
                : null,
            }
          : null,
      };
      if (deps.quarantine) deps.quarantine(record);
      else if (existsSync(cwd) && !lstatSync(cwd).isSymbolicLink()) {
        const target = path.join(
          cwd,
          `tool-cleanup-quarantine-${child.pid ?? "unknown"}-${Date.now()}.json`,
        );
        const encoded = `${JSON.stringify(record, null, 2)}\n`;
        if (Buffer.byteLength(encoded, "utf8") > 4096)
          fail("tool-cleanup-unproven");
        writeFileSync(target, encoded, { flag: "wx" });
      }
    };
    const finish = (code) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (settlementTimer) clearTimeout(settlementTimer);
      child.stdout.removeListener("data", take);
      child.stderr.removeListener("data", take);
      if (terminal || code !== 0)
        reject(new Error(`playwright-cli-${terminal ?? `exit-${code}`}`));
      else resolve(Buffer.concat(chunks).toString("utf8"));
    };
    const terminate = (reason) => {
      if (terminal) return;
      terminal = reason;
      let cleanup;
      try {
        cleanup = (
          deps.cleanupIdentity ??
          ((owned) => processIdentityHelper("cleanup", owned))
        )(identity);
      } catch {
        cleanup = null;
      }
      if (!cleanup || cleanup.identityLost || cleanup.remaining?.length !== 0) {
        terminal = "cleanup-unproven";
        quarantine(reason, cleanup);
      }
      settlementTimer = setTimeout(() => finish(null), 250);
    };
    const take = (chunk) => {
      if (terminal) return;
      bytes += chunk.length;
      if (bytes > maximumOutputBytes) {
        terminate("output-oversize");
        return;
      }
      chunks.push(Buffer.from(chunk));
    };
    child.stdout.on("data", take);
    child.stderr.on("data", take);
    try {
      identity = (
        deps.captureIdentity ?? ((pid) => processIdentityHelper("capture", pid))
      )(child.pid);
      if (!identity?.processId || !identity?.creationDate)
        fail("tool-cleanup-unproven");
    } catch {
      terminal = "cleanup-unproven";
      quarantine("identity-capture-failed");
      settlementTimer = setTimeout(() => finish(null), 250);
    }
    child.on("error", (error) => {
      terminate(`spawn-${error.code ?? "failed"}`);
    });
    const timer = setTimeout(() => {
      terminate("timeout");
    }, remaining);
    child.on("close", finish);
  });
}

function parsePlaywrightValue(output) {
  const trimmed = output.trim();
  try {
    return JSON.parse(trimmed);
  } catch {
    // CLI diagnostics may precede the final JSON value.
  }
  if (trimmed === "true") return true;
  if (trimmed === "false") return false;
  const start = Math.max(
    trimmed.lastIndexOf("\n{"),
    trimmed.lastIndexOf("\n["),
    0,
  );
  try {
    return JSON.parse(trimmed.slice(start).trim());
  } catch {
    return trimmed;
  }
}

export async function attachPlaywright(options, deadline, deps = {}) {
  const runTool = deps.runTool ?? runBoundedTool;
  const session = `sage-ide-${process.pid}-${Date.now()}`;
  const base = ["--session", session];
  await runTool(
    options.playwrightCli,
    [...base, "attach", "--cdp", `http://127.0.0.1:${options.port}`],
    deadline,
    options.output,
  );
  return {
    evaluate: async (expression) =>
      parsePlaywrightValue(
        await runTool(
          options.playwrightCli,
          [...base, "eval", expression],
          deadline,
          options.output,
        ),
      ),
    inspectTarget: async () =>
      parsePlaywrightValue(
        await runTool(
          options.playwrightCli,
          [
            ...base,
            "run-code",
            'async page => { const session = await page.context().newCDPSession(page); try { const { targetInfo } = await session.send("Target.getTargetInfo"); return { id: targetInfo.targetId, type: targetInfo.type, title: targetInfo.title, url: targetInfo.url }; } finally { await session.detach(); } }',
          ],
          deadline,
          options.output,
        ),
      ),
    screenshot: async (name) => {
      const encoded = parsePlaywrightValue(
        await runTool(
          options.playwrightCli,
          [
            ...base,
            "run-code",
            "async page => (await page.screenshot()).toString('base64')",
          ],
          deadline,
          options.output,
          { maxOutputBytes: Math.ceil((MAX_ARTIFACT_BYTES * 4) / 3) + 4096 },
        ),
      );
      const bytes = decodeBoundedScreenshot(String(encoded));
      const target = path.join(options.output, name);
      writeContainedBytes(options.sourceRoot, options.output, name, bytes);
      return target;
    },
    close: async () => {
      await runTool(
        options.playwrightCli,
        [...base, "close"],
        Date.now() + 5000,
        options.output,
      );
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

const UI_ACTIONS = Object.freeze([
  action(
    "Project",
    "document.querySelectorAll('[aria-label=\"Projects and chats\"]').length===1&&document.querySelectorAll('[aria-label=\"Project files\"]').length===1",
    "Project root is not visible",
  ),
  action(
    "Explorer",
    "(()=>{const x=[...document.querySelectorAll('button[aria-label=\"Explorer\"]')];if(x.length!==1)return false;x[0].click();return true})()",
    "Explorer control is ambiguous",
  ),
  action(
    "Search",
    "(()=>{const x=[...document.querySelectorAll('button[aria-label=\"Search\"]')];if(x.length!==1)return false;x[0].click();return true})()",
    "Search control is ambiguous",
  ),
  action(
    "File",
    "(()=>{const input=document.querySelector('[aria-label=\"Search files\"]');if(!input)return false;const set=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;set.call(input,'SAGE_JOURNEY_SENTINEL');input.dispatchEvent(new Event('input',{bubbles:true}));const button=[...document.querySelectorAll('button')].find(x=>x.textContent.trim()==='Search');if(!button)return false;button.click();return true})()",
    "Search form is unavailable",
  ),
  action(
    "edit-diagnostic",
    "(()=>{const x=[...document.querySelectorAll('[aria-label^=\"Open journey.ts at line\"]')];if(x.length!==1)return false;x[0].click();return true})()",
    "Search result was not unique",
  ),
  action(
    "save",
    "(()=>{const x=[...document.querySelectorAll('button')].find(x=>x.textContent.trim()==='Save');if(!x)return false;x.click();return true})()",
    "Save control is unavailable",
  ),
  action(
    "Source Control",
    "(()=>{const x=[...document.querySelectorAll('button[aria-label=\"Source Control\"]')];if(x.length!==1)return false;x[0].click();return true})()",
    "Source Control control is ambiguous",
  ),
  action(
    "Diff",
    "(()=>{const x=[...document.querySelectorAll('button[aria-label=\"Open diff for journey.ts\"]')];if(x.length!==1)return false;x[0].click();return true})()",
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

function verificationFor(step) {
  if (step === "Explorer")
    return 'document.querySelectorAll(\'[role="tree"][aria-label="Project files"] [aria-label="Open file journey.ts"]\').length===1';
  if (step === "Search")
    return "document.querySelectorAll('[aria-label=\"Search files\"]').length===1";
  if (step === "File")
    return "document.querySelectorAll('[aria-label^=\"Open journey.ts at line\"]').length===1";
  if (step === "edit-diagnostic")
    return "document.querySelectorAll('[aria-label=\"File editor journey.ts\"]').length===1";
  if (step === "save")
    return "document.querySelectorAll('[aria-label=\"File editor journey.ts\"]').length===1";
  if (step === "Source Control")
    return "document.querySelectorAll('[aria-label=\"Source Control project tools\"]').length===1";
  if (step === "Diff")
    return "document.querySelectorAll('[aria-label=\"Diff for journey.ts\"]').length===1";
  return UI_ACTIONS[0].expression;
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
  let controlledTarget = null;
  try {
    controlledTarget = await revalidateAttachedPage(
      options,
      cdp,
      admittedTarget,
      deadline,
    );
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
        if (
          !(await cdp.evaluate(
            'document.querySelectorAll(\'[aria-label="File editor journey.ts"] .cm-content[contenteditable="true"]\').length===1',
          ))
        )
          throw new Error(
            "editor did not expose one editable CodeMirror surface",
          );
        if (
          (await cdp.evaluate(
            '(()=>{const e=document.querySelector(\'[aria-label="File editor journey.ts"] .cm-content[contenteditable="true"]\');e.focus();return document.execCommand("insertText",false,"\\n// journey edited through Playwright\\n")})()',
          )) !== true
        )
          throw new Error("editor did not accept native text insertion");
        if (
          (await cdp.evaluate(
            "(()=>{const x=[...document.querySelectorAll('button[aria-label=\"Problems\"]')];if(x.length!==1)return false;x[0].click();return true})()",
          )) !== true
        )
          throw new Error("Problems control is ambiguous");
        await waitFor(
          cdp.evaluate,
          "document.querySelectorAll('[aria-label=\"Problems\"] .problem-card').length>=1",
          deadline,
          "language diagnostic did not arrive",
        );
        diagnosticMessages = await cdp.evaluate(
          "JSON.stringify([...document.querySelectorAll('[aria-label=\"Problems\"] .problem-card')].map(x=>x.innerText))",
        );
        await cdp.evaluate(
          "(()=>{const x=[...document.querySelectorAll('button[aria-label=\"Search\"]')];if(x.length!==1)return false;x[0].click();return true})()",
        );
      }
      if (current.label === "save") {
        await waitFor(
          cdp.evaluate,
          "(()=>{const x=[...document.querySelectorAll('button')].find(x=>x.textContent.trim()==='Save');return x?.disabled===true})()",
          deadline,
          "save did not settle to a disabled control",
        );
        savedContent = await cdp.evaluate(
          "document.querySelector('[aria-label=\"File editor journey.ts\"] .cm-content')?.innerText??''",
        );
      }
      if (current.label === "Diff") {
        diffContent = await cdp.evaluate(
          "document.querySelector('[aria-label=\"Diff for journey.ts\"]')?.innerText??''",
        );
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
    const receipt = createJourneyReceipt({
      source: { commit: options.sourceCommit, tree: options.sourceTree },
      executable: {
        path: options.executable,
        ...validateExecutableBinding(options.executable, executableBinding),
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
