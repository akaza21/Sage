import {
  existsSync,
  lstatSync,
  readFileSync,
  realpathSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  runBoundedTool,
  validateExecutableBinding,
  validatePlainCanonicalExecutable,
} from "./source-functional-native-ide-journey.mjs";
import {
  attestNativeDiagnostics,
  createFinalAcp04Receipt,
  validatePendingAcp04Candidate,
} from "./source-functional-acp04-evidence.mjs";

export {
  attestNativeDiagnostics,
  createFinalAcp04Receipt,
  validatePendingAcp04Candidate,
  validateRuntimeObservation,
} from "./source-functional-acp04-evidence.mjs";

const CODEX_VERSION = /^codex(?:-cli)?\s+(\d+\.\d+\.\d+)$/u;
const CMD_UNSAFE_PATH_CHARACTER = /["%!^&|<>()\r\n\0]/u;

function fail(code) {
  throw new Error(`acp04-${code}`);
}

function absolute(value) {
  return (
    typeof value === "string" &&
    (path.isAbsolute(value) || path.win32.isAbsolute(value))
  );
}

function exactKeys(value, keys) {
  return (
    value &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    Object.keys(value).sort().join("\0") === [...keys].sort().join("\0")
  );
}

function canonicalCommandInterpreter(value) {
  if (!absolute(value) || path.basename(value).toLowerCase() !== "cmd.exe")
    fail("codex-preflight-invalid");
  const canonical = (realpathSync.native ?? realpathSync)(value);
  if (
    path.normalize(canonical).toLowerCase() !==
    path.normalize(value).toLowerCase()
  )
    fail("codex-preflight-invalid");
  for (let current = canonical; ; current = path.dirname(current)) {
    if (lstatSync(current).isSymbolicLink()) fail("codex-preflight-invalid");
    if (path.dirname(current) === current) return canonical;
  }
}

function commandInterpreter(deps = {}) {
  if (deps.command) return canonicalCommandInterpreter(deps.command);
  const root = process.env.SystemRoot ?? process.env.WINDIR;
  return canonicalCommandInterpreter(
    path.join(root ?? "C:\\Windows", "System32", "cmd.exe"),
  );
}

function validateCodexPreflightPath(options, deps = {}) {
  if (
    typeof options?.codex?.path !== "string" ||
    CMD_UNSAFE_PATH_CHARACTER.test(options.codex.path)
  )
    fail("codex-preflight-invalid");
  (deps.validateCodex ?? validatePlainCanonicalExecutable)(
    options.codex.path,
    options.codex,
  );
}

async function runCodexVersion(options, deadline, deps = {}) {
  validateCodexPreflightPath(options, deps);
  const command = commandInterpreter(deps);
  const output = await (deps.runTool ?? runBoundedTool)(
    command,
    [
      "/d",
      "/s",
      "/c",
      `set /p "SAGE_ACP04_ACK=" && call "${options.codex.path}" --version`,
    ],
    deadline,
    options.output,
    {
      commandRole: "codex-version-preflight",
      expectedExecutablePath: command,
      identityHandshake: "captured-owner\r\n",
      maxOutputBytes: 4096,
      windowsVerbatimArguments: true,
    },
  );
  const display = output.trim();
  const semanticVersion = CODEX_VERSION.exec(display)?.[1];
  if (!semanticVersion) fail("codex-preflight-invalid");
  return { display, semanticVersion };
}

export async function preflightCodex(options, deadline, deps = {}) {
  const version = await runCodexVersion(options, deadline, deps);
  return Object.freeze({
    ...options,
    codex: Object.freeze({
      ...options.codex,
      version: version.display,
      semanticVersion: version.semanticVersion,
    }),
  });
}

export async function revalidateSource(options, deadline, deps = {}) {
  const runTool = deps.runTool ?? runBoundedTool;
  const command = commandInterpreter(deps);
  const sourceToolOptions = (commandRole) => ({
    commandRole,
    expectedExecutablePath: command,
    identityHandshake: "captured-owner\r\n",
    maxOutputBytes: 4096,
    spawnCwd: options.sourceRoot,
    windowsVerbatimArguments: true,
  });
  const status = await runTool(
    command,
    [
      "/d",
      "/s",
      "/c",
      'set /p "SAGE_ACP04_ACK=" && git status --porcelain=v1 --untracked-files=all',
    ],
    deadline,
    options.output,
    sourceToolOptions("source-status"),
  );
  if (status !== "") fail("source-dirty");
  const identity = await runTool(
    command,
    [
      "/d",
      "/s",
      "/c",
      'set /p "SAGE_ACP04_ACK=" && git rev-parse HEAD HEAD^{tree}',
    ],
    deadline,
    options.output,
    sourceToolOptions("source-identity"),
  );
  const lines = identity.trim().split(/\r?\n/u);
  if (
    lines.length !== 2 ||
    lines[0] !== options.source.commit ||
    lines[1] !== options.source.tree
  )
    fail("source-identity-churn");
}

export async function revalidateCodexPreflight(options, deadline, deps = {}) {
  const version = await runCodexVersion(options, deadline, deps);
  if (
    version.display !== options.codex.version ||
    version.semanticVersion !== options.codex.semanticVersion
  )
    fail("codex-preflight-invalid");
}

export async function revalidateCollectionIdentities(
  options,
  deadline,
  deps = {},
) {
  await revalidateSource(options, deadline, deps);
  validateExecutableBinding(options.executable.path, options.executable);
  validatePlainCanonicalExecutable(process.execPath, options.node);
  await revalidateCodexPreflight(options, deadline, deps);
}

export async function finalizeAcp04Collection({
  closeCdp,
  revalidateIdentities,
  createReceipt,
  writePendingReceipt,
}) {
  await closeCdp();
  await revalidateIdentities();
  const receipt = createReceipt();
  writePendingReceipt(receipt);
  return receipt;
}

function parseFinalizerArguments(argv) {
  if (!Array.isArray(argv) || argv.length % 2 !== 0)
    fail("finalizer-arguments");
  const values = Object.create(null);
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index]?.replace(/^--/u, "");
    if (!key || key in values) fail("finalizer-arguments");
    values[key] = argv[index + 1];
  }
  const keys = [
    "pending",
    "final",
    "source-root",
    "stdout",
    "stderr",
    "sage-cleanup",
    "vite-cleanup",
    "deadline-ms",
  ];
  const deadlineMs = Number(values["deadline-ms"]);
  if (
    !exactKeys(values, keys) ||
    ![
      values.pending,
      values.final,
      values["source-root"],
      values.stdout,
      values.stderr,
    ].every(absolute) ||
    !Number.isSafeInteger(deadlineMs) ||
    deadlineMs < 1_000 ||
    deadlineMs > 60_000
  )
    fail("finalizer-arguments");
  const directory = path.dirname(values.pending);
  if (
    path.dirname(values.final) !== directory ||
    path.dirname(values.stdout) !== directory ||
    path.dirname(values.stderr) !== directory ||
    path.basename(values.pending) !== "machine-receipt.pending.json" ||
    path.basename(values.final) !== "machine-receipt.json" ||
    existsSync(values.final)
  )
    fail("finalizer-containment");
  return { ...values, deadlineMs };
}

function decodeCleanup(encoded, role) {
  let value;
  try {
    value = JSON.parse(Buffer.from(encoded, "base64").toString("utf8"));
  } catch {
    fail("cleanup-invalid");
  }
  if (
    !exactKeys(value, ["role", "identityLost", "remainingCount"]) ||
    value.role !== role ||
    value.identityLost !== false ||
    value.remainingCount !== 0
  )
    fail("cleanup-invalid");
  return Object.freeze({ role, identityLost: false, remainingCount: 0 });
}

export function decodeTerminalZeroCleanup(sageEncoded, viteEncoded) {
  return Object.freeze({
    sage: decodeCleanup(sageEncoded, "sage"),
    vite: decodeCleanup(viteEncoded, "vite"),
  });
}

function readPending(filePath) {
  if (
    !existsSync(filePath) ||
    lstatSync(filePath).isSymbolicLink() ||
    lstatSync(filePath).size > 1024 * 1024
  )
    fail("pending-invalid");
  try {
    return JSON.parse(readFileSync(filePath, "utf8"));
  } catch {
    fail("pending-invalid");
  }
}

async function executeFinalizer(options) {
  const candidate = validatePendingAcp04Candidate(
    readPending(options.pending),
    path.dirname(options.pending),
  );
  const cleanup = decodeTerminalZeroCleanup(
    options["sage-cleanup"],
    options["vite-cleanup"],
  );
  const diagnostics = attestNativeDiagnostics(options.stdout, options.stderr);
  validatePlainCanonicalExecutable(
    candidate.runtimeObservation.admittedEntrypoint.canonicalPath,
    {
      bytes: candidate.runtimeObservation.admittedEntrypoint.bytes,
      sha256: candidate.runtimeObservation.admittedEntrypoint.sha256,
    },
  );
  validatePlainCanonicalExecutable(
    candidate.runtimeObservation.retainedRootProcess.canonicalImagePath,
    {
      bytes: candidate.runtimeObservation.retainedRootProcess.bytes,
      sha256: candidate.runtimeObservation.retainedRootProcess.sha256,
    },
  );
  await revalidateCollectionIdentities(
    {
      sourceRoot: options["source-root"],
      output: path.dirname(options.pending),
      source: candidate.source,
      executable: candidate.identities.sageExecutable,
      node: candidate.identities.node,
      codex: candidate.identities.codexPreflight,
    },
    Date.now() + options.deadlineMs,
  );
  writeFileSync(
    options.final,
    `${JSON.stringify(createFinalAcp04Receipt(candidate, diagnostics, cleanup), null, 2)}\n`,
    { flag: "wx" },
  );
  unlinkSync(options.pending);
  return options.final;
}

if (
  process.argv[1] &&
  path.resolve(fileURLToPath(import.meta.url)) === path.resolve(process.argv[1])
) {
  try {
    await executeFinalizer(parseFinalizerArguments(process.argv.slice(2)));
  } catch (error) {
    console.error(String(error?.message ?? error).slice(0, 1024));
    process.exitCode = 1;
  }
}
