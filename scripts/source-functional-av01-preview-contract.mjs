import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import path from "node:path";

import { hashFile } from "./source-functional-native-ide-journey.mjs";
import {
  AV01_PREVIEW_CASES,
  readAv01PreviewFixtureIdentity,
} from "./source-functional-av01-preview-fixture.mjs";

const SHA256 = /^[a-f0-9]{64}$/u;
const GIT_OBJECT = /^[a-f0-9]{40,64}$/u;
const RECEIPT_NAME = /^machine-receipt\.pending\.json$/u;
const MAX_ARTIFACT_BYTES = 16 * 1024 * 1024;
const ISO_UTC_MILLIS = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u;

function fail(code) {
  throw new Error(`av01-preview-${code}`);
}

function absolute(value) {
  return (
    typeof value === "string" &&
    (path.isAbsolute(value) || path.win32.isAbsolute(value))
  );
}

function positiveInteger(value, maximum = Number.MAX_SAFE_INTEGER) {
  if (!(
    (typeof value === "string" && /^[1-9]\d*$/u.test(value)) ||
    (typeof value === "number" && Number.isSafeInteger(value) && value > 0)
  ))
    return null;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed > 0 && parsed <= maximum
    ? parsed
    : null;
}

function exactKeys(value, keys) {
  return (
    value &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    Object.keys(value).sort().join("\0") === [...keys].sort().join("\0")
  );
}

function validFixtureProcessIdentity(value, expected) {
  return (
    exactKeys(value, [
      "schemaVersion",
      "nonce",
      "processId",
      "executablePath",
      "creationDate",
      "sageProcessId",
    ]) &&
    value.schemaVersion === 1 &&
    value.nonce === expected.nonce &&
    Number.isSafeInteger(value.processId) &&
    value.processId > 0 &&
    value.processId <= 0x7fffffff &&
    absolute(value.executablePath) &&
    ISO_UTC_MILLIS.test(value.creationDate ?? "") &&
    Number.isSafeInteger(value.sageProcessId) &&
    value.sageProcessId === expected.sageProcessId
  );
}

export function validateAv01FixtureProcessCapture(value, expected) {
  if (
    !expected ||
    !SHA256.test(expected.nonce ?? "") ||
    positiveInteger(expected.sageProcessId, 0x7fffffff) === null ||
    !validFixtureProcessIdentity(value, expected)
  )
    fail("fixture-process-invalid");
  return Object.freeze({ ...value });
}

export function validateAv01FixtureProcessAbsence(value, capture) {
  if (
    !capture ||
    !exactKeys(value, [
      "schemaVersion",
      "absent",
      "nonce",
      "processId",
      "executablePath",
      "creationDate",
      "sageProcessId",
    ]) ||
    value.schemaVersion !== 1 ||
    value.absent !== true ||
    !validFixtureProcessIdentity(
      {
        schemaVersion: value.schemaVersion,
        nonce: value.nonce,
        processId: value.processId,
        executablePath: value.executablePath,
        creationDate: value.creationDate,
        sageProcessId: value.sageProcessId,
      },
      capture,
    ) ||
    [
      "nonce",
      "processId",
      "executablePath",
      "creationDate",
      "sageProcessId",
    ].some((key) => value[key] !== capture[key])
  )
    fail("fixture-process-absence-invalid");
  return Object.freeze({ ...value });
}

function parsePairs(argv) {
  if (!Array.isArray(argv) || argv.length % 2 !== 0) fail("arguments-invalid");
  const raw = Object.create(null);
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    if (typeof key !== "string" || !key.startsWith("--") || key.slice(2) in raw)
      fail("arguments-invalid");
    raw[key.slice(2)] = argv[index + 1];
  }
  return raw;
}

export function parseAv01PreviewArguments(argv) {
  const raw = parsePairs(argv);
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
    "powershell-path",
    "powershell-bytes",
    "powershell-sha256",
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
    "fixture",
    "fixture-identity",
    "fixture-process-helper",
    "preview-case",
  ]);
  const port = positiveInteger(raw.port, 65535);
  const deadlineMs = positiveInteger(raw["deadline-ms"], 180_000);
  const executableBytes = positiveInteger(
    raw["executable-bytes"],
    256 * 1024 * 1024,
  );
  const nodeBytes = positiveInteger(raw["node-bytes"], 256 * 1024 * 1024);
  const powershellBytes = positiveInteger(
    raw["powershell-bytes"],
    256 * 1024 * 1024,
  );
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
    port === null ||
    port < 1024 ||
    deadlineMs === null ||
    deadlineMs < 30_000 ||
    raw["preview-case"] === undefined ||
    !AV01_PREVIEW_CASES.includes(raw["preview-case"]) ||
    !GIT_OBJECT.test(raw["source-commit"] ?? "") ||
    !GIT_OBJECT.test(raw["source-tree"] ?? "") ||
    ![
      raw.output,
      raw.executable,
      raw["source-root"],
      raw["codex-path"],
      raw["powershell-path"],
      raw["profile-root"],
      raw["profile-identity-artifact"],
      raw["sage-stdout-log"],
      raw["sage-stderr-log"],
      raw.fixture,
      raw["fixture-identity"],
      raw["fixture-process-helper"],
    ].every(absolute) ||
    executableBytes === null ||
    nodeBytes === null ||
    powershellBytes === null ||
    codexBytes === null ||
    processId === null ||
    ![
      raw["executable-sha256"],
      raw["node-sha256"],
      raw["codex-sha256"],
      raw["powershell-sha256"],
    ].every((value) => SHA256.test(value ?? "")) ||
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
    !RECEIPT_NAME.test(raw["receipt-name"] ?? "") ||
    path.basename(raw["fixture-identity"]) !== "fixture-identity.json" ||
    path.dirname(raw["fixture-identity"]) !== raw.fixture
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
    powershell: {
      path: raw["powershell-path"],
      bytes: powershellBytes,
      sha256: raw["powershell-sha256"],
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
    fixture: {
      path: raw.fixture,
      identityPath: raw["fixture-identity"],
      case: raw["preview-case"],
      processHelper: raw["fixture-process-helper"],
    },
  });
}

function artifact(output, name) {
  const filePath = path.join(output, name);
  if (!existsSync(filePath)) fail("receipt-invalid");
  const identity = hashFile(filePath, MAX_ARTIFACT_BYTES);
  return Object.freeze({ name, ...identity });
}

function stateHash(value) {
  if (typeof value !== "string" || value.length < 1 || value.length > 512)
    fail("receipt-invalid");
  return createHash("sha256").update(value).digest("hex");
}

function exactFileIdentity(expected) {
  if (
    !expected?.path ||
    !positiveInteger(expected.bytes) ||
    !SHA256.test(expected.sha256)
  )
    fail("receipt-invalid");
  const actual = hashFile(expected.path, 256 * 1024 * 1024);
  if (actual.bytes !== expected.bytes || actual.sha256 !== expected.sha256)
    fail("receipt-invalid");
  return expected;
}

function validManualOrigin(value) {
  try {
    const origin = new URL(value);
    return (
      origin.protocol === "http:" &&
      origin.hostname === "127.0.0.1" &&
      origin.port.length > 0 &&
      !origin.username &&
      !origin.password &&
      !origin.search &&
      !origin.hash
    );
  } catch {
    return false;
  }
}

function validatePreviewProcessEvidence(preview, expected) {
  if (!preview || typeof preview !== "object") fail("receipt-invalid");
  if (preview.case === "starting-active") {
    const capture = validateAv01FixtureProcessCapture(
      preview.fixtureProcess,
      expected,
    );
    validateAv01FixtureProcessAbsence(preview.fixtureRetirement, capture);
    if (
      preview.finalPhase !== "active" ||
      preview.manualOrigin !== null ||
      typeof preview.activeFrameUrl !== "string" ||
      !validManualOrigin(preview.activeFrameUrl)
    )
      fail("receipt-invalid");
    return;
  }
  if (
    preview.fixtureProcess !== null ||
    preview.fixtureRetirement !== null ||
    preview.activeFrameUrl !== null ||
    (preview.case === "health-failed" &&
      (preview.finalPhase !== "healthFailed" ||
        !validManualOrigin(preview.manualOrigin))) ||
    (preview.case === "exited" &&
      (preview.finalPhase !== "exited" || preview.manualOrigin !== null)) ||
    !["health-failed", "exited"].includes(preview.case)
  )
    fail("receipt-invalid");
}

export function createAv01PreviewReceipt(
  options,
  result,
  target,
  artifacts,
  cdpStats,
) {
  const fixture = readAv01PreviewFixtureIdentity(
    options?.fixture?.path,
    options?.fixture?.case,
  );
  if (
    !result ||
    result.modeA?.state?.phase !== "complete" ||
    !result.preview ||
    result.preview.case !== options.fixture.case ||
    !target?.id ||
    !Array.isArray(artifacts) ||
    artifacts.length !== 2 ||
    artifacts[0] !== "av01-preview-observation.json" ||
    artifacts[1] !== "av01-preview.png" ||
    cdpStats?.connectionCount !== 1 ||
    cdpStats?.maxInFlight !== 1 ||
    !positiveInteger(cdpStats?.requestCount, 4096)
  )
    fail("receipt-invalid");
  validatePreviewProcessEvidence(result.preview, {
    nonce: fixture.nonce,
    sageProcessId: options.sageProcess.processId,
  });
  exactFileIdentity(options.powershell);
  return Object.freeze({
    schemaVersion: 1,
    completed: true,
    evidenceClass: "source-functional-native-preview",
    excludedClaims: Object.freeze([
      "package-release",
      "macOS acceptance",
      "AV02 acceptance",
      "Q04 performance acceptance",
    ]),
    source: options.source,
    startedAt: result.startedAt,
    completedAt: result.completedAt,
    identities: {
      sageExecutable: options.executable,
      sageProcess: options.sageProcess,
      node: options.node,
      codexPreflight: options.codex,
      helperRuntime: options.powershell,
    },
    runtime: {
      mode: options.runtimeMode,
      frontendReady: options.frontendReady,
    },
    fixture: {
      case: fixture.case,
      nonceSha256: stateHash(fixture.nonce),
      files: fixture.files,
      processHelper: hashFile(
        options.fixture.processHelper,
        MAX_ARTIFACT_BYTES,
      ),
    },
    cdp: {
      port: options.port,
      targetId: target.id,
      origin: options.expectedOrigin,
      connectionCount: cdpStats.connectionCount,
      maxInFlight: cdpStats.maxInFlight,
      requestCount: cdpStats.requestCount,
    },
    owner: {
      projectIdSha256: stateHash(result.modeA.state.projectId),
      chatIdSha256: stateHash(result.modeA.state.chatId),
      sessionIdSha256: stateHash(result.modeA.state.sessionId),
      runtimeEpoch: result.modeA.state.runtimeEpoch,
    },
    preview: result.preview,
    artifacts: artifacts.map((name) => artifact(options.output, name)),
  });
}

export function validatePendingAv01PreviewReceipt(value, output) {
  if (
    !exactKeys(value, [
      "schemaVersion",
      "completed",
      "evidenceClass",
      "excludedClaims",
      "source",
      "startedAt",
      "completedAt",
      "identities",
      "runtime",
      "fixture",
      "cdp",
      "owner",
      "preview",
      "artifacts",
    ]) ||
    value.schemaVersion !== 1 ||
    value.completed !== true ||
    value.evidenceClass !== "source-functional-native-preview" ||
    !GIT_OBJECT.test(value.source?.commit ?? "") ||
    !GIT_OBJECT.test(value.source?.tree ?? "") ||
    !Array.isArray(value.artifacts) ||
    value.artifacts.length !== 2 ||
    value.artifacts.some(
      ({ name, bytes, sha256 }) =>
        !["av01-preview-observation.json", "av01-preview.png"].includes(name) ||
        !Number.isSafeInteger(bytes) ||
        bytes < 1 ||
        bytes > MAX_ARTIFACT_BYTES ||
        !SHA256.test(sha256),
    ) ||
    value.artifacts.some(
      (entry) =>
        JSON.stringify(entry) !== JSON.stringify(artifact(output, entry.name)),
    )
  )
    fail("pending-invalid");
  try {
    const preview = value.preview;
    const expected = {
      nonce:
        preview?.fixtureProcess?.nonce ??
        preview?.fixtureRetirement?.nonce ??
        "",
      sageProcessId: value.identities?.sageProcess?.processId,
    };
    if (
      preview?.case === "starting-active" &&
      stateHash(expected.nonce) !== value.fixture?.nonceSha256
    )
      fail("pending-invalid");
    validatePreviewProcessEvidence(preview, expected);
  } catch {
    fail("pending-invalid");
  }
  return Object.freeze(value);
}
