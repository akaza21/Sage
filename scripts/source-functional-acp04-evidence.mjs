import { createHash } from "node:crypto";
import {
  existsSync,
  lstatSync,
  readFileSync,
  realpathSync,
  statSync,
} from "node:fs";
import path from "node:path";

import { hashFile } from "./source-functional-native-ide-journey.mjs";

const SHA256 = /^[a-f0-9]{64}$/u;
const GIT_OBJECT = /^[a-f0-9]{40,64}$/u;
const DECIMAL = /^(?:0|[1-9]\d*)$/u;
const PROFILE_MARKER = ".sage-disposable-profile-v1";
const PROFILE_MARKER_BYTES = "sage-disposable-profile-v1\n";
const MAX_NATIVE_LOG_BYTES = 1024 * 1024;
const MAX_TEXT_BYTES = 4096;

function fail(code) {
  throw new Error(`acp04-${code}`);
}

function exactKeys(value, keys) {
  return (
    value &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    Object.keys(value).sort().join("\0") === [...keys].sort().join("\0")
  );
}

function absolute(value) {
  return (
    typeof value === "string" &&
    (path.isAbsolute(value) || path.win32.isAbsolute(value))
  );
}

function samePath(left, right) {
  const normalize =
    process.platform === "win32"
      ? (value) => value.toLowerCase()
      : (value) => value;
  return normalize(path.normalize(left)) === normalize(path.normalize(right));
}

function bounded(value, maximum = MAX_TEXT_BYTES) {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    Buffer.byteLength(value, "utf8") <= maximum &&
    ![...value].some((character) => /\p{Cc}/u.test(character))
  );
}

function positiveInteger(value, maximum = Number.MAX_SAFE_INTEGER) {
  return Number.isSafeInteger(value) && value > 0 && value <= maximum;
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function validIdentity(value) {
  return (
    exactKeys(value, ["path", "bytes", "sha256"]) &&
    absolute(value.path) &&
    positiveInteger(value.bytes, 512 * 1024 * 1024) &&
    SHA256.test(value.sha256)
  );
}

function validSelection(value) {
  return (
    exactKeys(value, ["id", "text"]) &&
    bounded(value.id) &&
    bounded(value.text, 256)
  );
}

function validArtifact(value, name, output) {
  const artifact = path.join(output, name);
  return (
    exactKeys(value, ["name", "bytes", "sha256"]) &&
    value.name === name &&
    positiveInteger(value.bytes, 16 * 1024 * 1024) &&
    SHA256.test(value.sha256) &&
    JSON.stringify(hashFile(artifact)) ===
      JSON.stringify({ bytes: value.bytes, sha256: value.sha256 })
  );
}

export function revalidateFreshProfile(profile, output) {
  if (
    !exactKeys(profile, ["path", "identityArtifact"]) ||
    !absolute(profile.path) ||
    !absolute(profile.identityArtifact) ||
    path.basename(profile.identityArtifact) !== "acp04-profile-identity.json" ||
    !samePath(path.dirname(profile.identityArtifact), output)
  )
    fail("profile-identity-invalid");
  const root = realpathSync(profile.path);
  const evidenceRoot = realpathSync(output);
  const rootInfo = lstatSync(root);
  if (
    rootInfo.isSymbolicLink() ||
    !rootInfo.isDirectory() ||
    !samePath(path.dirname(root), evidenceRoot)
  )
    fail("profile-identity-invalid");
  const marker = path.join(root, PROFILE_MARKER);
  if (
    !existsSync(marker) ||
    lstatSync(marker).isSymbolicLink() ||
    !statSync(marker).isFile() ||
    readFileSync(marker, "utf8") !== PROFILE_MARKER_BYTES
  )
    fail("profile-identity-invalid");
  const artifact = hashFile(profile.identityArtifact, 4096);
  let declared;
  try {
    declared = JSON.parse(readFileSync(profile.identityArtifact, "utf8"));
  } catch {
    fail("profile-identity-invalid");
  }
  const identitySha256 = sha256(root.replace(/[\\/]+$/u, "").toLowerCase());
  if (
    !exactKeys(declared, [
      "schemaVersion",
      "profileRoot",
      "profileIdentitySha256",
    ]) ||
    declared.schemaVersion !== 1 ||
    !samePath(declared.profileRoot, root) ||
    declared.profileIdentitySha256 !== identitySha256
  )
    fail("profile-identity-invalid");
  return Object.freeze({
    path: root,
    identityArtifact: {
      name: path.basename(profile.identityArtifact),
      ...artifact,
    },
    identitySha256,
  });
}

export function attestNativeDiagnostics(stdoutPath, stderrPath) {
  const inspect = (filePath) => {
    const identity = hashFile(filePath, MAX_NATIVE_LOG_BYTES);
    const text = readFileSync(filePath, "utf8");
    const failure =
      /(?:host[_ ]?error\s*=\s*closed|HostError::Closed|protocol_failure_reason=ProtocolFailure|\bACP\b[^\n]{0,80}\b(?:recovery|capacity)\b)/iu;
    if (text.split(/\r?\n/u).some((line) => failure.test(line)))
      fail("native-diagnostic-failure");
    return Object.freeze({ ...identity });
  };
  return Object.freeze({
    stdout: inspect(stdoutPath),
    stderr: inspect(stderrPath),
  });
}

export function validateRuntimeObservation(value, expectedOwner) {
  if (
    !exactKeys(value, ["owner", "admittedEntrypoint", "retainedRootProcess"]) ||
    !exactKeys(value.owner, [
      "projectId",
      "chatId",
      "sessionId",
      "runtimeEpoch",
    ]) ||
    !exactKeys(value.admittedEntrypoint, [
      "canonicalPath",
      "sha256",
      "bytes",
      "version",
      "bindingDigest",
      "kind",
    ]) ||
    !exactKeys(value.retainedRootProcess, [
      "processId",
      "creationTimeFiletime100ns",
      "canonicalImagePath",
      "sha256",
      "bytes",
    ]) ||
    value.owner.projectId !== expectedOwner.projectId ||
    value.owner.chatId !== expectedOwner.chatId ||
    value.owner.sessionId !== expectedOwner.sessionId ||
    !bounded(value.owner.projectId) ||
    !bounded(value.owner.chatId) ||
    !bounded(value.owner.sessionId) ||
    !DECIMAL.test(value.owner.runtimeEpoch) ||
    value.owner.runtimeEpoch !== String(expectedOwner.runtimeEpoch) ||
    !absolute(value.admittedEntrypoint.canonicalPath) ||
    !SHA256.test(value.admittedEntrypoint.sha256) ||
    !positiveInteger(value.admittedEntrypoint.bytes, 512 * 1024 * 1024) ||
    !bounded(value.admittedEntrypoint.version, 256) ||
    !SHA256.test(value.admittedEntrypoint.bindingDigest) ||
    !["windowsBatchLauncher", "nativeExecutable"].includes(
      value.admittedEntrypoint.kind,
    ) ||
    !positiveInteger(value.retainedRootProcess.processId, 0x7fffffff) ||
    !DECIMAL.test(value.retainedRootProcess.creationTimeFiletime100ns) ||
    !absolute(value.retainedRootProcess.canonicalImagePath) ||
    !SHA256.test(value.retainedRootProcess.sha256) ||
    !positiveInteger(value.retainedRootProcess.bytes, 512 * 1024 * 1024)
  )
    fail("runtime-observation-invalid");
  return Object.freeze({
    owner: Object.freeze({ ...value.owner }),
    admittedEntrypoint: Object.freeze({ ...value.admittedEntrypoint }),
    retainedRootProcess: Object.freeze({ ...value.retainedRootProcess }),
  });
}

export function createFinalAcp04Receipt(candidate, diagnostics, cleanup) {
  const owner = candidate?.runtimeObservation?.owner;
  if (
    !candidate ||
    candidate.completed !== true ||
    candidate.evidenceClass !== "source-functional-collection-only" ||
    candidate.sourceBinding !== "collection-only" ||
    !candidate.excludedClaims?.includes("ACP-04 acceptance") ||
    !owner ||
    sha256(owner.projectId) !== candidate.observation?.projectIdSha256 ||
    sha256(owner.chatId) !== candidate.observation?.chatIdSha256 ||
    sha256(owner.sessionId) !== candidate.observation?.sessionIdSha256 ||
    String(owner.runtimeEpoch) !==
      String(candidate.observation?.runtimeEpoch) ||
    !diagnostics?.stdout ||
    !diagnostics?.stderr ||
    cleanup?.sage?.remainingCount !== 0 ||
    cleanup?.vite?.remainingCount !== 0
  )
    fail("final-receipt-invalid");
  return Object.freeze({
    ...candidate,
    nativeDiagnostics: diagnostics,
    cleanup,
    finalizedAt: new Date().toISOString(),
  });
}

export function validatePendingAcp04Candidate(candidate, output) {
  const expectedArtifacts = [
    "acp04-mode-a-observations.json",
    "acp04-mode-a-complete.png",
  ];
  const expectedExcluded = [
    "ACP-04 acceptance",
    "package-c0-b",
    "uia-state-plan",
    "paint-receipt",
  ];
  const schemaInvalid =
    !exactKeys(candidate, [
      "schemaVersion",
      "completed",
      "evidenceClass",
      "sourceBinding",
      "mode",
      "excludedClaims",
      "source",
      "startedAt",
      "completedAt",
      "identities",
      "runtime",
      "cdp",
      "observation",
      "runtimeObservation",
      "artifacts",
    ]) ||
    candidate.schemaVersion !== 1 ||
    candidate.completed !== true ||
    candidate.evidenceClass !== "source-functional-collection-only" ||
    candidate.sourceBinding !== "collection-only" ||
    candidate.mode !== "two-turn" ||
    !Array.isArray(candidate.excludedClaims) ||
    candidate.excludedClaims.join("\0") !== expectedExcluded.join("\0") ||
    !exactKeys(candidate.source, ["commit", "tree"]) ||
    !GIT_OBJECT.test(candidate.source.commit) ||
    !GIT_OBJECT.test(candidate.source.tree) ||
    !bounded(candidate.startedAt, 128) ||
    !bounded(candidate.completedAt, 128) ||
    Number.isNaN(Date.parse(candidate.startedAt)) ||
    Number.isNaN(Date.parse(candidate.completedAt)) ||
    !exactKeys(candidate.identities, [
      "sageExecutable",
      "sageProcess",
      "node",
      "codexPreflight",
      "profile",
    ]) ||
    !validIdentity(candidate.identities.sageExecutable) ||
    !validIdentity(candidate.identities.node) ||
    !exactKeys(candidate.identities.codexPreflight, [
      "path",
      "bytes",
      "sha256",
      "version",
      "semanticVersion",
    ]) ||
    !absolute(candidate.identities.codexPreflight.path) ||
    !positiveInteger(
      candidate.identities.codexPreflight.bytes,
      512 * 1024 * 1024,
    ) ||
    !SHA256.test(candidate.identities.codexPreflight.sha256) ||
    !bounded(candidate.identities.codexPreflight.version, 256) ||
    !/^\d+\.\d+\.\d+$/u.test(
      candidate.identities.codexPreflight.semanticVersion,
    ) ||
    !exactKeys(candidate.identities.sageProcess, [
      "processId",
      "creationDate",
    ]) ||
    !positiveInteger(candidate.identities.sageProcess.processId, 0x7fffffff) ||
    !bounded(candidate.identities.sageProcess.creationDate, 128) ||
    !exactKeys(candidate.runtime, ["mode", "frontendReady"]) ||
    candidate.runtime.mode !== "tauri-dev-split" ||
    candidate.runtime.frontendReady !== true ||
    !exactKeys(candidate.cdp, [
      "port",
      "targetId",
      "origin",
      "connectionCount",
      "maxInFlight",
      "requestCount",
    ]) ||
    !positiveInteger(candidate.cdp.port, 65535) ||
    !bounded(candidate.cdp.targetId) ||
    candidate.cdp.connectionCount !== 1 ||
    candidate.cdp.maxInFlight !== 1 ||
    !positiveInteger(candidate.cdp.requestCount, 1024) ||
    !validLoopbackOrigin(candidate.cdp.origin) ||
    !exactKeys(candidate.observation, [
      "projectIdSha256",
      "chatIdSha256",
      "sessionIdSha256",
      "turnIds",
      "turnOrdinals",
      "itemIds",
      "projectionRevision",
      "runtimeEpoch",
      "harness",
      "model",
      "echoes",
    ]) ||
    ![
      candidate.observation.projectIdSha256,
      candidate.observation.chatIdSha256,
      candidate.observation.sessionIdSha256,
    ].every((item) => SHA256.test(item)) ||
    ![
      candidate.observation.projectionRevision,
      candidate.observation.runtimeEpoch,
    ].every((item) => DECIMAL.test(item)) ||
    !validSelection(candidate.observation.harness) ||
    !validSelection(candidate.observation.model) ||
    !Array.isArray(candidate.observation.turnIds) ||
    candidate.observation.turnIds.length !== 2 ||
    !candidate.observation.turnIds.every((item) => bounded(item)) ||
    new Set(candidate.observation.turnIds).size !== 2 ||
    !Array.isArray(candidate.observation.turnOrdinals) ||
    candidate.observation.turnOrdinals.length !== 2 ||
    candidate.observation.turnOrdinals.some(
      (item, index) =>
        !positiveInteger(item, 1024) ||
        (index > 0 && item <= candidate.observation.turnOrdinals[index - 1]),
    ) ||
    !Array.isArray(candidate.observation.itemIds) ||
    candidate.observation.itemIds.length !== 4 ||
    !candidate.observation.itemIds.every((item) => bounded(item)) ||
    new Set(candidate.observation.itemIds).size !== 4 ||
    !Array.isArray(candidate.observation.echoes) ||
    candidate.observation.echoes.join("\0") !==
      "SAGE_ACP04_A_TURN_1\0SAGE_ACP04_A_TURN_2" ||
    !Array.isArray(candidate.artifacts) ||
    candidate.artifacts.length !== expectedArtifacts.length ||
    !expectedArtifacts.every((name, index) =>
      validArtifact(candidate.artifacts[index], name, output),
    );
  if (schemaInvalid) fail("pending-schema-invalid");
  const profile = revalidateFreshProfile(
    {
      path: candidate.identities.profile?.path,
      identityArtifact: path.join(
        output,
        candidate.identities.profile?.identityArtifact?.name ?? "",
      ),
    },
    output,
  );
  if (JSON.stringify(profile) !== JSON.stringify(candidate.identities.profile))
    fail("profile-identity-churn");
  const runtime = validateRuntimeObservation(
    candidate.runtimeObservation,
    candidate.runtimeObservation?.owner ?? {},
  );
  if (
    runtime.admittedEntrypoint.version !==
      candidate.identities.codexPreflight.version ||
    runtime.admittedEntrypoint.version !==
      candidate.observation.harness.text.replace(/^Codex \u2014 /u, "") ||
    sha256(runtime.owner.projectId) !== candidate.observation.projectIdSha256 ||
    sha256(runtime.owner.chatId) !== candidate.observation.chatIdSha256 ||
    sha256(runtime.owner.sessionId) !== candidate.observation.sessionIdSha256 ||
    runtime.owner.runtimeEpoch !== candidate.observation.runtimeEpoch
  )
    fail("runtime-version-mismatch");
  return Object.freeze({ ...candidate, runtimeObservation: runtime });
}

function validLoopbackOrigin(value) {
  try {
    const origin = new URL(value);
    return (
      origin.protocol === "http:" &&
      origin.hostname === "127.0.0.1" &&
      origin.pathname === "/" &&
      !origin.username &&
      !origin.password &&
      !origin.search &&
      !origin.hash
    );
  } catch {
    return false;
  }
}
