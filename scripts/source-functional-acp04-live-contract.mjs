import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import path from "node:path";

import { hashFile } from "./source-functional-native-ide-journey.mjs";
import { revalidateFreshProfile } from "./source-functional-acp04-evidence.mjs";

export const ACP04_MODE_A = "two-turn";
export const ACP04_ECHOES = Object.freeze([
  "SAGE_ACP04_A_TURN_1",
  "SAGE_ACP04_A_TURN_2",
]);

const SHA256 = /^[a-f0-9]{64}$/u;
const GIT_OBJECT = /^[a-f0-9]{40,64}$/u;
const RECEIPT_NAME = /^machine-receipt\.pending(?:-[a-z0-9-]{1,32})?\.json$/u;

function fail(code) {
  throw new Error(`acp04-${code}`);
}

function absolute(value) {
  return (
    typeof value === "string" &&
    (path.isAbsolute(value) || path.win32.isAbsolute(value))
  );
}

function positiveInteger(value, maximum = Number.MAX_SAFE_INTEGER) {
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed > 0 && parsed <= maximum
    ? parsed
    : null;
}

export function codexSemanticVersion(value) {
  if (typeof value !== "string" || value.length > 128) return null;
  return /^codex(?:-cli)?\s+(\d+\.\d+\.\d+)$/u.exec(value.trim())?.[1] ?? null;
}

export function parseAcp04ProbeArguments(argv) {
  if (!Array.isArray(argv) || argv.length % 2 !== 0) fail("arguments-invalid");
  const raw = Object.create(null);
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    if (typeof key !== "string" || !key.startsWith("--") || key.slice(2) in raw)
      fail("arguments-invalid");
    raw[key.slice(2)] = argv[index + 1];
  }
  const known = new Set([
    "port",
    "output",
    "source-commit",
    "source-tree",
    "executable",
    "source-root",
    "executable-bytes",
    "executable-sha256",
    "node-bytes",
    "node-sha256",
    "codex-path",
    "codex-bytes",
    "codex-sha256",
    "profile-root",
    "profile-identity-artifact",
    "sage-stdout-log",
    "sage-stderr-log",
    "sage-process-id",
    "sage-process-created",
    "expected-origin",
    "runtime-mode",
    "frontend-ready",
    "receipt-name",
    "deadline-ms",
    "mode",
  ]);
  const port = positiveInteger(raw.port, 65535);
  const deadlineMs = positiveInteger(raw["deadline-ms"], 180_000);
  const executableBytes = positiveInteger(
    raw["executable-bytes"],
    256 * 1024 * 1024,
  );
  const nodeBytes = positiveInteger(raw["node-bytes"], 256 * 1024 * 1024);
  const codexBytes = positiveInteger(raw["codex-bytes"], 256 * 1024 * 1024);
  const processId = positiveInteger(raw["sage-process-id"], 0x7fffffff);
  let origin;
  try {
    origin = new URL(raw["expected-origin"]);
  } catch {
    fail("arguments-invalid");
  }
  if (
    Object.keys(raw).some((key) => !known.has(key)) ||
    port < 1024 ||
    deadlineMs < 15_000 ||
    raw.mode !== ACP04_MODE_A ||
    !GIT_OBJECT.test(raw["source-commit"] ?? "") ||
    !GIT_OBJECT.test(raw["source-tree"] ?? "") ||
    ![
      raw.output,
      raw.executable,
      raw["source-root"],
      raw["codex-path"],
      raw["profile-root"],
      raw["profile-identity-artifact"],
      raw["sage-stdout-log"],
      raw["sage-stderr-log"],
    ].every(absolute) ||
    executableBytes === null ||
    nodeBytes === null ||
    codexBytes === null ||
    processId === null ||
    ![raw["executable-sha256"], raw["node-sha256"], raw["codex-sha256"]].every(
      (value) => SHA256.test(value ?? ""),
    ) ||
    typeof raw["sage-process-created"] !== "string" ||
    raw["sage-process-created"].length < 1 ||
    raw["sage-process-created"].length > 128 ||
    origin.protocol !== "http:" ||
    origin.hostname !== "127.0.0.1" ||
    origin.username ||
    origin.password ||
    origin.search ||
    origin.hash ||
    origin.pathname !== "/" ||
    raw["runtime-mode"] !== "tauri-dev-split" ||
    raw["frontend-ready"] !== "true" ||
    !RECEIPT_NAME.test(raw["receipt-name"] ?? "")
  )
    fail("arguments-invalid");
  return Object.freeze({
    port,
    output: raw.output,
    sourceRoot: raw["source-root"],
    source: { commit: raw["source-commit"], tree: raw["source-tree"] },
    executable: {
      path: raw.executable,
      bytes: executableBytes,
      sha256: raw["executable-sha256"],
    },
    node: {
      path: process.execPath,
      bytes: nodeBytes,
      sha256: raw["node-sha256"],
    },
    codex: {
      path: raw["codex-path"],
      bytes: codexBytes,
      sha256: raw["codex-sha256"],
    },
    profile: {
      path: raw["profile-root"],
      identityArtifact: raw["profile-identity-artifact"],
    },
    nativeLogs: {
      stdout: raw["sage-stdout-log"],
      stderr: raw["sage-stderr-log"],
    },
    sageProcess: { processId, creationDate: raw["sage-process-created"] },
    expectedOrigin: origin.href,
    runtimeMode: raw["runtime-mode"],
    frontendReady: true,
    receiptName: raw["receipt-name"],
    deadlineMs,
    mode: raw.mode,
  });
}

export function createAcp04Receipt(
  options,
  result,
  target,
  artifacts,
  cdpStats,
) {
  const profile = revalidateFreshProfile(options?.profile, options?.output);
  if (
    !result ||
    result.state?.phase !== "complete" ||
    !result.runtimeObservation ||
    !positiveInteger(options?.sageProcess?.processId, 0x7fffffff) ||
    typeof options?.sageProcess?.creationDate !== "string" ||
    typeof target?.id !== "string" ||
    target.id.length < 1 ||
    !Array.isArray(artifacts) ||
    artifacts.length !== 2 ||
    path.basename(artifacts[0] ?? "") !== "acp04-mode-a-observations.json" ||
    path.basename(artifacts[1] ?? "") !== "acp04-mode-a-complete.png" ||
    !artifacts.every(
      (artifact) => absolute(artifact) && existsSync(artifact),
    ) ||
    cdpStats?.connectionCount !== 1 ||
    cdpStats?.maxInFlight !== 1 ||
    !positiveInteger(cdpStats?.requestCount, 1024)
  )
    fail("receipt-invalid");
  return Object.freeze({
    schemaVersion: 1,
    completed: true,
    evidenceClass: "source-functional-collection-only",
    sourceBinding: "collection-only",
    mode: options.mode,
    excludedClaims: Object.freeze([
      "ACP-04 acceptance",
      "package-c0-b",
      "uia-state-plan",
      "paint-receipt",
    ]),
    source: options.source,
    startedAt: result.startedAt,
    completedAt: result.completedAt,
    identities: {
      sageExecutable: options.executable,
      sageProcess: options.sageProcess,
      node: options.node,
      codexPreflight: options.codex,
      profile,
    },
    runtime: {
      mode: options.runtimeMode,
      frontendReady: options.frontendReady,
    },
    cdp: {
      port: options.port,
      targetId: target.id,
      origin: options.expectedOrigin,
      connectionCount: cdpStats.connectionCount,
      maxInFlight: cdpStats.maxInFlight,
      requestCount: cdpStats.requestCount,
    },
    observation: {
      projectIdSha256: createHash("sha256")
        .update(result.state.projectId)
        .digest("hex"),
      chatIdSha256: createHash("sha256")
        .update(result.state.chatId)
        .digest("hex"),
      sessionIdSha256: createHash("sha256")
        .update(result.state.sessionId)
        .digest("hex"),
      turnIds: result.state.turnIds,
      turnOrdinals: result.state.turnOrdinals,
      itemIds: result.state.itemIds,
      projectionRevision: result.state.projectionRevision,
      runtimeEpoch: result.state.runtimeEpoch,
      harness: result.state.harness,
      model: result.state.model,
      echoes: result.state.echoes,
    },
    runtimeObservation: result.runtimeObservation,
    artifacts: artifacts.map((artifact) => ({
      name: path.basename(artifact),
      ...hashFile(artifact),
    })),
  });
}
