import { createHash } from "node:crypto";
import {
  closeSync,
  existsSync,
  fstatSync,
  lstatSync,
  openSync,
  readSync,
} from "node:fs";
import path from "node:path";

import { hashFile } from "./source-functional-native-ide-journey.mjs";
import {
  AV01_PREVIEW_CASES,
  readAv01PreviewFixtureIdentity,
} from "./source-functional-av01-preview-fixture.mjs";
import { createReadOnlyFileReceiptProof } from "./source-functional-acp04-file-turn.mjs";

const SHA256 = /^[a-f0-9]{64}$/u;
const GIT_OBJECT = /^[a-f0-9]{40,64}$/u;
const RECEIPT_NAME = /^machine-receipt\.pending\.json$/u;
const MAX_ARTIFACT_BYTES = 16 * 1024 * 1024;
// The observation is structured metadata, not a screenshot or transcript.
const MAX_OBSERVATION_BYTES = 512 * 1024;
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
    "git-path",
    "git-bytes",
    "git-sha256",
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
  const gitBytes = positiveInteger(raw["git-bytes"], 256 * 1024 * 1024);
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
      raw["git-path"],
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
    gitBytes === null ||
    processId === null ||
    ![
      raw["executable-sha256"],
      raw["node-sha256"],
      raw["codex-sha256"],
      raw["git-sha256"],
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
    git: { path: raw["git-path"], bytes: gitBytes, sha256: raw["git-sha256"] },
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

function matchesArtifact(output, entry) {
  try {
    return (
      JSON.stringify(entry) === JSON.stringify(artifact(output, entry.name))
    );
  } catch {
    return false;
  }
}

function stateHash(value) {
  if (typeof value !== "string" || value.length < 1 || value.length > 512)
    fail("receipt-invalid");
  return createHash("sha256").update(value).digest("hex");
}

function sameRecord(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function validFixtureGit(value) {
  return (
    exactKeys(value, ["commit", "tree"]) &&
    GIT_OBJECT.test(value.commit ?? "") &&
    GIT_OBJECT.test(value.tree ?? "")
  );
}

function validCapturedFixture(value) {
  const names = [
    "av01-preview-case.json",
    "package.json",
    "preview-fixture.mjs",
  ];
  return (
    exactKeys(value, ["schemaVersion", "case", "nonce", "files"]) &&
    value.schemaVersion === 1 &&
    AV01_PREVIEW_CASES.includes(value.case) &&
    SHA256.test(value.nonce ?? "") &&
    Array.isArray(value.files) &&
    value.files.length === names.length &&
    value.files.map((entry) => entry?.name).join("\0") === names.join("\0") &&
    value.files.every(
      (entry) =>
        exactKeys(entry, ["name", "bytes", "sha256"]) &&
        names.includes(entry.name) &&
        Number.isSafeInteger(entry.bytes) &&
        entry.bytes > 0 &&
        entry.bytes <= 64 * 1024 &&
        SHA256.test(entry.sha256 ?? ""),
    )
  );
}

const OBSERVATION_IO = Object.freeze({
  closeSync,
  fstatSync,
  lstatSync,
  openSync,
  readSync,
});

function readCapturedObservation(output, declared, io = OBSERVATION_IO) {
  const outputPath = path.resolve(output);
  const observationPath = path.resolve(
    outputPath,
    "av01-preview-observation.json",
  );
  if (
    path.dirname(observationPath) !== outputPath ||
    declared?.name !== "av01-preview-observation.json"
  )
    fail("pending-invalid");
  let metadata;
  try {
    metadata = io.lstatSync(observationPath);
  } catch {
    fail("pending-invalid");
  }
  if (
    metadata.isSymbolicLink() ||
    !metadata.isFile() ||
    metadata.size < 1 ||
    metadata.size > MAX_OBSERVATION_BYTES
  )
    fail("pending-invalid");
  let bytes;
  let descriptor;
  try {
    descriptor = io.openSync(observationPath, "r");
    const opened = io.fstatSync(descriptor);
    if (
      !opened.isFile() ||
      opened.size !== metadata.size ||
      opened.dev !== metadata.dev ||
      opened.ino !== metadata.ino
    )
      fail("pending-invalid");
    const buffer = Buffer.allocUnsafe(MAX_OBSERVATION_BYTES + 1);
    let length = 0;
    while (length < buffer.length) {
      const read = io.readSync(
        descriptor,
        buffer,
        length,
        buffer.length - length,
        length,
      );
      if (read === 0) break;
      length += read;
    }
    const after = io.fstatSync(descriptor);
    const pathAfter = io.lstatSync(observationPath);
    if (
      length !== metadata.size ||
      after.size !== metadata.size ||
      after.dev !== metadata.dev ||
      after.ino !== metadata.ino ||
      pathAfter.isSymbolicLink() ||
      !pathAfter.isFile() ||
      pathAfter.size !== metadata.size ||
      pathAfter.dev !== metadata.dev ||
      pathAfter.ino !== metadata.ino
    )
      fail("pending-invalid");
    bytes = buffer.subarray(0, length);
  } catch {
    fail("pending-invalid");
  } finally {
    if (descriptor !== undefined) io.closeSync(descriptor);
  }
  const sha256 = createHash("sha256").update(bytes).digest("hex");
  if (
    bytes.length !== metadata.size ||
    bytes.length !== declared.bytes ||
    sha256 !== declared.sha256
  )
    fail("pending-invalid");
  try {
    return JSON.parse(bytes.toString("utf8"));
  } catch {
    fail("pending-invalid");
  }
}

function capturedObservation(output, declared) {
  return readCapturedObservation(output, declared);
}

// The production contract always uses OBSERVATION_IO.  This narrow seam makes
// descriptor identity and short-read behavior deterministic in its node tests.
export function readAv01PreviewObservationForTest(output, declared, io) {
  return readCapturedObservation(output, declared, io);
}

function validateCapturedObservation(result, value) {
  if (
    !exactKeys(result, [
      "startedAt",
      "completedAt",
      "modeA",
      "fileProof",
      "fixtureBefore",
      "fixtureAfter",
      "gitBefore",
      "gitAfter",
      "preview",
    ]) ||
    !validCapturedFixture(result.fixtureBefore) ||
    !sameRecord(result.fixtureBefore, result.fixtureAfter) ||
    result.fixtureBefore.case !== value.fixture?.case ||
    !sameRecord(result.fixtureBefore.files, value.fixture?.files) ||
    stateHash(result.fixtureBefore.nonce) !== value.fixture?.nonceSha256 ||
    !validFixtureGit(result.gitBefore) ||
    !sameRecord(result.gitBefore, result.gitAfter) ||
    !sameRecord(result.gitBefore, value.fixtureGit) ||
    result.startedAt !== value.startedAt ||
    result.completedAt !== value.completedAt ||
    !sameRecord(result.preview, value.preview) ||
    !result.modeA?.state ||
    stateHash(result.modeA.state.projectId ?? "") !==
      value.owner?.projectIdSha256 ||
    stateHash(result.modeA.state.chatId ?? "") !== value.owner?.chatIdSha256 ||
    stateHash(result.modeA.state.sessionId ?? "") !==
      value.owner?.sessionIdSha256 ||
    result.modeA.state.runtimeEpoch !== value.owner?.runtimeEpoch
  )
    fail("pending-invalid");
  const proof = createBoundFileProof(result, result.fixtureBefore);
  if (
    !sameRecord(proof, value.fileProof) ||
    !sameRecord(proof.fixtureGit, value.fixtureGit) ||
    !sameRecord(proof.observations.gitBefore, value.fixtureGit) ||
    !sameRecord(proof.observations.gitAfter, value.fixtureGit)
  )
    fail("pending-invalid");
}

function createBoundFileProof(result, fixture) {
  if (
    !sameRecord(result.fixtureBefore, fixture) ||
    !sameRecord(result.fixtureAfter, fixture) ||
    !validFixtureGit(result.gitBefore) ||
    !sameRecord(result.gitBefore, result.gitAfter)
  )
    fail("receipt-invalid");
  const proof = createReadOnlyFileReceiptProof(result.fileProof);
  return Object.freeze({
    ...proof,
    fixtureFilesSha256: stateHash(JSON.stringify(fixture.files)),
    fixtureGit: Object.freeze({ ...result.gitAfter }),
    observations: Object.freeze({
      fixtureBeforeSha256: stateHash(
        JSON.stringify(result.fixtureBefore.files),
      ),
      fixtureAfterSha256: stateHash(JSON.stringify(result.fixtureAfter.files)),
      gitBefore: Object.freeze({ ...result.gitBefore }),
      gitAfter: Object.freeze({ ...result.gitAfter }),
    }),
  });
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
    !result.fileProof ||
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
  const fileProof = createBoundFileProof(result, fixture);
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
    fixtureGit: Object.freeze({ ...result.gitAfter }),
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
    fileProof,
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
      "fixtureGit",
      "cdp",
      "owner",
      "fileProof",
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
    new Set(value.artifacts.map(({ name }) => name)).size !== 2 ||
    !["av01-preview-observation.json", "av01-preview.png"].every((name) =>
      value.artifacts.some((entry) => entry.name === name),
    ) ||
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
        entry.name !== "av01-preview-observation.json" &&
        !matchesArtifact(output, entry),
    )
  )
    fail("pending-invalid");
  try {
    const observationArtifact = value.artifacts.find(
      ({ name }) => name === "av01-preview-observation.json",
    );
    validateCapturedObservation(
      capturedObservation(output, observationArtifact),
      value,
    );
    const fileProof = value.fileProof;
    if (
      !fileProof ||
      !exactKeys(fileProof, [
        "actionEnabled",
        "fixtureFilesSha256",
        "fixtureGit",
        "itemIdSha256",
        "observations",
        "observedAt",
        "owner",
        "resource",
        "schemaVersion",
        "selectedReadyFile",
        "turnIdSha256",
      ]) ||
      fileProof.schemaVersion !== 1 ||
      !SHA256.test(fileProof.turnIdSha256 ?? "") ||
      !SHA256.test(fileProof.itemIdSha256 ?? "") ||
      !exactKeys(fileProof.owner, [
        "chatIdSha256",
        "projectIdSha256",
        "runtimeEpoch",
        "sessionIdSha256",
      ]) ||
      fileProof.owner?.projectIdSha256 !== value.owner?.projectIdSha256 ||
      fileProof.owner?.chatIdSha256 !== value.owner?.chatIdSha256 ||
      fileProof.owner?.sessionIdSha256 !== value.owner?.sessionIdSha256 ||
      fileProof.owner?.runtimeEpoch !== value.owner?.runtimeEpoch ||
      !exactKeys(fileProof.resource, ["relativePath", "rootIdSha256"]) ||
      !SHA256.test(fileProof.resource?.rootIdSha256 ?? "") ||
      !SHA256.test(fileProof.fixtureFilesSha256 ?? "") ||
      !validFixtureGit(fileProof.fixtureGit) ||
      !exactKeys(fileProof.observations, [
        "fixtureAfterSha256",
        "fixtureBeforeSha256",
        "gitAfter",
        "gitBefore",
      ]) ||
      !SHA256.test(fileProof.observations.fixtureBeforeSha256 ?? "") ||
      !SHA256.test(fileProof.observations.fixtureAfterSha256 ?? "") ||
      !validFixtureGit(fileProof.observations.gitBefore) ||
      !validFixtureGit(fileProof.observations.gitAfter) ||
      fileProof.resource?.relativePath !== "package.json" ||
      fileProof.actionEnabled !== true ||
      fileProof.selectedReadyFile !== true ||
      !ISO_UTC_MILLIS.test(fileProof.observedAt ?? "")
    )
      fail("pending-invalid");
    const fixtureFilesSha256 = stateHash(JSON.stringify(value.fixture?.files));
    if (
      !validFixtureGit(value.fixtureGit) ||
      fileProof.fixtureFilesSha256 !== fixtureFilesSha256 ||
      fileProof.observations.fixtureBeforeSha256 !== fixtureFilesSha256 ||
      fileProof.observations.fixtureAfterSha256 !== fixtureFilesSha256 ||
      !sameRecord(fileProof.fixtureGit, value.fixtureGit) ||
      !sameRecord(fileProof.observations.gitBefore, value.fixtureGit) ||
      !sameRecord(fileProof.observations.gitAfter, value.fixtureGit)
    )
      fail("pending-invalid");
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
