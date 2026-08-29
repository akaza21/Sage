#!/usr/bin/env node
// ACP-05 collection helper.  It observes an already-running, loopback-only
// WebView; it never starts, stops, or authenticates that WebView.
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const ACP05_MARKER = "ACP renderer recreation pairing passed";
export const RENDERER_RECREATION_PACKET_EVAL =
  '(function(){try{var collect=window.__SAGE_COLLECT_RENDERER_RECREATION_PACKET__;if(typeof collect!=="function")return null;return collect();}catch(_error){return null;}})()';
export const SCHEMA_VERSION = 2;
export const KIND = "acp05-native-reload-capture";
export const LIMITS = Object.freeze({
  deadlineMs: 60_000,
  maxTargets: 32,
  maxDiscoveryBytes: 65_536,
  maxPacketBytes: 512,
  maxMarkerBytes: 4_096,
  maxLogBytes: 4 * 1024 * 1024,
  maxScreenshotBytes: 8 * 1024 * 1024,
  maxExecutableBytes: 512 * 1024 * 1024,
  pollMs: 100,
});

const HEX40 = /^[a-f0-9]{40}$/u;
const HEX64 = /^[a-f0-9]{64}$/u;
const LOOPBACK = new Set(["127.0.0.1", "localhost", "::1", "[::1]"]);
const FOCUS_OUTCOMES = new Set([
  "exact-sage-id",
  "activity-summary-fallback",
  "no-restoration-when-no-valid-envelope",
]);
const MARKER_PROOFS = new Set([
  "native-pairing-marker",
  "launcher-attested-owned-stdout",
]);

export class Acp05Error extends Error {
  constructor(reason, detail = "") {
    super(`acp05-${reason}${detail ? `: ${detail}` : ""}`);
    this.reason = reason;
  }
}

const bytes = (value) => Buffer.byteLength(value, "utf8");
const absolute = (value) =>
  typeof value === "string" && value.length > 0 && path.isAbsolute(value);
const hash = (value) => crypto.createHash("sha256").update(value).digest("hex");

export function assertWithinRoot(root, candidate, allowRoot = false) {
  const base = path.resolve(root);
  const resolved = path.resolve(candidate);
  const relative = path.relative(base, resolved);
  if (
    (!allowRoot && relative === "") ||
    relative === ".." ||
    relative.startsWith(`..${path.sep}`) ||
    path.isAbsolute(relative)
  ) {
    throw new Acp05Error("path-escape");
  }
  return resolved;
}

function plainDirectory(fsImpl, directory) {
  const stat = fsImpl.lstatSync(directory);
  if (!stat.isDirectory() || stat.isSymbolicLink())
    throw new Acp05Error("reparse-path", "directory must not be a link");
}

function normalizedLeafPath(value) {
  const resolved = path.resolve(value).replace(/^\\\\\?\\/u, "");
  return process.platform === "win32" ? resolved.toLowerCase() : resolved;
}

function assertPlainRegularLeaf(fsImpl, filePath, reason) {
  const stat = fsImpl.lstatSync(filePath);
  const realpath = fsImpl.realpathSync.native ?? fsImpl.realpathSync;
  const resolved = realpath(filePath);
  if (
    !stat.isFile() ||
    stat.isSymbolicLink() ||
    normalizedLeafPath(resolved) !== normalizedLeafPath(filePath)
  )
    throw new Acp05Error(reason, "leaf must not be a link or reparse point");
  return stat;
}

// Validate every existing parent.  On Windows Node reports both symlinks and
// junctions as symbolic links from lstat, so this is deliberately fail-closed.
function ensurePlainDirectory(fsImpl, directory) {
  const resolved = path.resolve(directory);
  const missing = [];
  let cursor = resolved;
  while (!fsImpl.existsSync(cursor)) {
    missing.push(cursor);
    const parent = path.dirname(cursor);
    if (parent === cursor)
      throw new Acp05Error("output-root", "no existing parent");
    cursor = parent;
  }
  plainDirectory(fsImpl, cursor);
  for (const next of missing.reverse()) {
    fsImpl.mkdirSync(next);
    plainDirectory(fsImpl, next);
  }
  return resolved;
}

function assertPlainExistingParents(fsImpl, root, candidate) {
  const resolved = assertWithinRoot(root, candidate, true);
  let cursor = path.dirname(resolved);
  const base = path.resolve(root);
  while (true) {
    if (!fsImpl.existsSync(cursor)) ensurePlainDirectory(fsImpl, cursor);
    plainDirectory(fsImpl, cursor);
    if (cursor === base) return resolved;
    const parent = path.dirname(cursor);
    if (parent === cursor) throw new Acp05Error("path-escape");
    cursor = parent;
  }
}

export function sha256File(filePath, fsImpl = fs) {
  const before = fsImpl.lstatSync(filePath);
  if (
    !before.isFile() ||
    before.isSymbolicLink() ||
    before.size > LIMITS.maxExecutableBytes
  )
    throw new Acp05Error("executable-invalid");
  const data = fsImpl.readFileSync(filePath);
  const after = fsImpl.lstatSync(filePath);
  if (after.size !== before.size || after.mtimeMs !== before.mtimeMs)
    throw new Acp05Error("executable-mutated");
  return { sha256: hash(data), bytes: data.length };
}

function endpoint(value) {
  let parsed;
  try {
    parsed = new URL(value);
  } catch {
    throw new Acp05Error("cdp-endpoint");
  }
  if (
    !["http:", "https:"].includes(parsed.protocol) ||
    !LOOPBACK.has(parsed.hostname) ||
    parsed.username ||
    parsed.password ||
    parsed.search ||
    parsed.hash ||
    !Number.isInteger(Number(parsed.port)) ||
    Number(parsed.port) < 1024 ||
    Number(parsed.port) > 65535 ||
    !["/", "/json"].includes(parsed.pathname)
  ) {
    throw new Acp05Error("cdp-endpoint");
  }
  return { url: parsed, port: Number(parsed.port) };
}

function webSocketForTarget(value, port) {
  let parsed;
  try {
    parsed = new URL(value);
  } catch {
    throw new Acp05Error("cdp-target", "invalid websocket URL");
  }
  if (
    !["ws:", "wss:"].includes(parsed.protocol) ||
    !LOOPBACK.has(parsed.hostname) ||
    Number(parsed.port) !== port ||
    parsed.username ||
    parsed.password ||
    parsed.search ||
    parsed.hash ||
    !parsed.pathname.startsWith("/devtools/page/")
  ) {
    throw new Acp05Error("cdp-target", "websocket escaped loopback port");
  }
  return parsed.toString();
}

export function parseArgs(argv) {
  if (!Array.isArray(argv) || argv.length % 2 !== 0)
    throw new Acp05Error("cli", "flags need one value");
  const optionNames = new Map([
    ["--repo-root", "repoRoot"],
    ["--source-commit", "sourceCommit"],
    ["--executable", "executablePath"],
    ["--executable-sha256", "executableSha256"],
    ["--cdp-endpoint", "cdpEndpoint"],
    ["--evidence-root", "evidenceRoot"],
    ["--marker-log", "markerLog"],
    ["--marker-proof", "markerProof"],
    ["--marker-nonce", "markerNonce"],
    ["--process-id", "processId"],
    ["--receipt", "receipt"],
    ["--deadline-ms", "deadlineMs"],
  ]);
  const result = Object.create(null);
  for (let index = 0; index < argv.length; index += 2) {
    const key = optionNames.get(argv[index]);
    const value = argv[index + 1];
    if (!key || typeof value !== "string" || key in result)
      throw new Acp05Error("cli", "unknown, duplicate, or missing flag");
    result[key] = value;
  }
  return result;
}

export function validateConfig(raw, fsImpl = fs) {
  if (!raw || typeof raw !== "object") throw new Acp05Error("config");
  for (const key of [
    "repoRoot",
    "sourceCommit",
    "executablePath",
    "executableSha256",
    "cdpEndpoint",
    "evidenceRoot",
    "markerLog",
  ]) {
    if (typeof raw[key] !== "string" || raw[key].length === 0)
      throw new Acp05Error("config", `missing ${key}`);
  }
  if (!absolute(raw.repoRoot) || !fsImpl.existsSync(raw.repoRoot))
    throw new Acp05Error("config", "repo root");
  plainDirectory(fsImpl, raw.repoRoot);
  if (!HEX40.test(raw.sourceCommit) || !HEX64.test(raw.executableSha256))
    throw new Acp05Error("config", "source identity");
  if (
    !absolute(raw.executablePath) ||
    !absolute(raw.evidenceRoot) ||
    !absolute(raw.markerLog)
  )
    throw new Acp05Error("config", "absolute paths required");
  const executable = sha256File(raw.executablePath, fsImpl);
  if (executable.sha256 !== raw.executableSha256)
    throw new Acp05Error("hash-mismatch", "executable");
  const cdp = endpoint(raw.cdpEndpoint);
  const evidenceBase = ensurePlainDirectory(
    fsImpl,
    path.join(path.resolve(raw.repoRoot), "local-evidence"),
  );
  const evidenceRoot = assertWithinRoot(evidenceBase, raw.evidenceRoot);
  ensurePlainDirectory(fsImpl, evidenceRoot);
  const markerParent = path.dirname(raw.markerLog);
  if (!fsImpl.existsSync(raw.markerLog))
    throw new Acp05Error(
      "marker-log",
      "must be an existing plain regular file",
    );
  assertPlainRegularLeaf(fsImpl, raw.markerLog, "marker-log");
  assertPlainExistingParents(
    fsImpl,
    path.parse(raw.markerLog).root,
    markerParent,
  );
  const receipt = raw.receipt ?? path.join(evidenceRoot, "acp05-receipt.json");
  if (!absolute(receipt)) throw new Acp05Error("config", "receipt path");
  assertPlainExistingParents(fsImpl, evidenceRoot, receipt);
  assertWithinRoot(evidenceRoot, receipt);
  if (fsImpl.existsSync(receipt))
    throw new Acp05Error("output-exists", "receipt");
  const deadlineMs =
    raw.deadlineMs === undefined ? LIMITS.deadlineMs : Number(raw.deadlineMs);
  if (
    !Number.isInteger(deadlineMs) ||
    deadlineMs < 1_000 ||
    deadlineMs > 180_000
  )
    throw new Acp05Error("config", "deadline-ms");
  if (
    raw.markerNonce !== undefined &&
    !/^[A-Za-z0-9._-]{8,128}$/u.test(raw.markerNonce)
  )
    throw new Acp05Error("config", "marker nonce");
  if (raw.processId !== undefined && !/^[1-9][0-9]{0,9}$/u.test(raw.processId))
    throw new Acp05Error("config", "process id");
  const markerProof = raw.markerProof ?? "native-pairing-marker";
  if (!MARKER_PROOFS.has(markerProof))
    throw new Acp05Error("config", "marker proof");
  if (
    markerProof === "launcher-attested-owned-stdout" &&
    (raw.markerNonce === undefined || raw.processId === undefined)
  )
    throw new Acp05Error("config", "launcher proof needs nonce and process id");
  return Object.freeze({
    repoRoot: path.resolve(raw.repoRoot),
    sourceCommit: raw.sourceCommit,
    executableSha256: raw.executableSha256,
    executableBytes: executable.bytes,
    cdpEndpoint: cdp.url.toString(),
    cdpPort: cdp.port,
    evidenceRoot,
    markerLog: path.resolve(raw.markerLog),
    markerProof,
    markerNonce: raw.markerNonce ?? null,
    processId: raw.processId ?? null,
    receipt: path.resolve(receipt),
    deadlineMs,
  });
}

function sleep(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function deadlineGate(now, deadlineMs) {
  const controller = new AbortController();
  const end = now() + deadlineMs;
  const remaining = () => Math.max(0, end - now());
  const timer = setTimeout(() => controller.abort(), deadlineMs);
  async function within(promise, label) {
    const left = remaining();
    if (left <= 0 || controller.signal.aborted)
      throw new Acp05Error("deadline", label);
    let timerId;
    try {
      return await Promise.race([
        promise,
        new Promise((_, reject) => {
          timerId = setTimeout(
            () => reject(new Acp05Error("deadline", label)),
            left,
          );
          controller.signal.addEventListener(
            "abort",
            () => reject(new Acp05Error("deadline", label)),
            { once: true },
          );
        }),
      ]);
    } finally {
      clearTimeout(timerId);
    }
  }
  return {
    signal: controller.signal,
    remaining,
    within,
    close: () => clearTimeout(timer),
  };
}

async function discoverTarget(config, fetchImpl, gate, sleepFn) {
  const discoverUrl = new URL("/json", config.cdpEndpoint).toString();
  let last = "not discovered";
  while (gate.remaining() > 0) {
    try {
      const response = await gate.within(
        fetchImpl(discoverUrl, { signal: gate.signal }),
        "discovery",
      );
      if (!response.ok) throw new Error(`status ${response.status}`);
      const text = await gate.within(response.text(), "discovery body");
      if (bytes(text) > LIMITS.maxDiscoveryBytes)
        throw new Error("discovery too large");
      const targets = JSON.parse(text);
      if (!Array.isArray(targets) || targets.length > LIMITS.maxTargets)
        throw new Error("discovery invalid");
      const admitted = targets.filter(
        (target) =>
          target &&
          target.type === "page" &&
          target.title === "Sage" &&
          target.url === "http://tauri.localhost/" &&
          typeof target.id === "string" &&
          target.id.length > 0 &&
          target.id.length <= 256 &&
          typeof target.webSocketDebuggerUrl === "string",
      );
      if (admitted.length === 1) {
        const target = admitted[0];
        return {
          id: target.id,
          title: target.title,
          url: target.url,
          webSocketDebuggerUrl: webSocketForTarget(
            target.webSocketDebuggerUrl,
            config.cdpPort,
          ),
        };
      }
      last = `admitted target count ${admitted.length}`;
    } catch (error) {
      if (error instanceof Acp05Error) throw error;
      last = String(error?.message ?? error).slice(0, 256);
    }
    await gate.within(
      sleepFn(Math.min(LIMITS.pollMs, gate.remaining())),
      "discovery retry",
    );
  }
  throw new Acp05Error("cdp-target", last);
}

export function createTransport(
  url,
  { WebSocketImpl = globalThis.WebSocket } = {},
) {
  if (typeof WebSocketImpl !== "function")
    throw new Acp05Error("websocket", "unavailable");
  const socket = new WebSocketImpl(url);
  const listeners = { open: [], error: [], message: [] };
  socket.addEventListener("open", () =>
    listeners.open.forEach((callback) => callback()),
  );
  socket.addEventListener("error", () =>
    listeners.error.forEach((callback) =>
      callback(new Error("CDP socket error")),
    ),
  );
  socket.addEventListener("message", ({ data }) => {
    try {
      const message = JSON.parse(data);
      listeners.message.forEach((callback) => callback(message));
    } catch {
      // Invalid CDP traffic cannot become evidence.
    }
  });
  return {
    onOpen: (callback) => listeners.open.push(callback),
    onError: (callback) => listeners.error.push(callback),
    onMessage: (callback) => listeners.message.push(callback),
    send: (message) => socket.send(JSON.stringify(message)),
    close: () => socket.close(),
  };
}

function createClient(transport, gate) {
  let nextId = 0;
  const pending = new Map();
  transport.onMessage((message) => {
    const waiter = pending.get(message?.id);
    if (!waiter) return;
    pending.delete(message.id);
    if (message.error) waiter.reject(new Acp05Error("cdp", "command failed"));
    else waiter.resolve(message.result);
  });
  transport.onError((error) => {
    for (const waiter of pending.values()) waiter.reject(error);
    pending.clear();
  });
  const call = (method, params = {}) =>
    gate.within(
      new Promise((resolve, reject) => {
        const id = ++nextId;
        pending.set(id, { resolve, reject });
        try {
          transport.send({ id, method, params });
        } catch (error) {
          pending.delete(id);
          reject(error);
        }
      }),
      `CDP ${method}`,
    );
  const evaluate = async (expression) => {
    const result = await call("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (result?.exceptionDetails || !("value" in (result?.result ?? {})))
      throw new Acp05Error("evaluate");
    return result.result.value;
  };
  return { call, evaluate, close: () => transport.close() };
}

function exactKeys(value, keys) {
  return (
    value &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    Object.keys(value).sort().join("\0") === [...keys].sort().join("\0")
  );
}

export function validatePacket(packet, phase) {
  if (
    !exactKeys(packet, [
      "schemaVersion",
      "liveRegions",
      "pagehideListeners",
      "focusOutcome",
      "projectionSlots",
    ]) ||
    packet.schemaVersion !== 1 ||
    !exactKeys(packet.liveRegions, ["polite", "assertive"]) ||
    packet.liveRegions.polite !== 1 ||
    packet.liveRegions.assertive !== 1 ||
    packet.pagehideListeners !== 1 ||
    ![1, 2].includes(packet.projectionSlots) ||
    !FOCUS_OUTCOMES.has(packet.focusOutcome) ||
    bytes(JSON.stringify(packet)) > LIMITS.maxPacketBytes
  ) {
    throw new Acp05Error("packet-invalid", phase);
  }
  return packet;
}

async function capturePacket(client, phase, loaderId) {
  const packet = validatePacket(
    await client.evaluate(RENDERER_RECREATION_PACKET_EVAL),
    phase,
  );
  return {
    packet,
    packetSha256: hash(Buffer.from(JSON.stringify(packet))),
    loaderSha256: hash(loaderId),
    capturedAt: new Date().toISOString(),
  };
}

async function waitForReadyDocument(client, previousLoaderId, gate, sleepFn) {
  while (gate.remaining() > 0) {
    const tree = await client.call("Page.getFrameTree");
    const loaderId = tree?.frameTree?.frame?.loaderId;
    const identity = await client.evaluate(
      "(()=>({title:document.title,href:location.href,readyState:document.readyState}))()",
    );
    if (
      typeof loaderId === "string" &&
      loaderId !== previousLoaderId &&
      identity?.title === "Sage" &&
      identity?.href === "http://tauri.localhost/" &&
      identity?.readyState === "complete"
    ) {
      return loaderId;
    }
    await gate.within(
      sleepFn(Math.min(LIMITS.pollMs, gate.remaining())),
      "new document readiness",
    );
  }
  throw new Acp05Error("reload-correlation");
}

function fileMarkerSource(logPath, fsImpl) {
  let identity = null;
  function inspect() {
    const stat = assertPlainRegularLeaf(fsImpl, logPath, "marker-log");
    if (stat.size > LIMITS.maxLogBytes)
      throw new Acp05Error("marker-log", "leaf is not a bounded plain file");
    return stat;
  }
  function sameFile(left, right) {
    return left.dev === right.dev && left.ino === right.ino;
  }
  function sameSnapshot(left, right) {
    return (
      sameFile(left, right) &&
      left.size === right.size &&
      left.mtimeMs === right.mtimeMs
    );
  }
  return {
    source: "file",
    start() {
      identity = inspect();
      return identity.size;
    },
    readFrom(offset) {
      const before = inspect();
      if (!identity || !sameFile(identity, before) || before.size < offset)
        throw new Acp05Error("marker-log", "rotated or oversized");
      const size = before.size - offset;
      const handle = fsImpl.openSync(logPath, "r");
      try {
        const opened = fsImpl.fstatSync(handle);
        if (!sameSnapshot(before, opened))
          throw new Acp05Error("marker-log", "leaf changed before read");
        const content = Buffer.alloc(size);
        let read = 0;
        while (read < size) {
          const count = fsImpl.readSync(
            handle,
            content,
            read,
            size - read,
            offset + read,
          );
          if (count === 0) throw new Acp05Error("marker-log", "short read");
          read += count;
        }
        const afterOpen = fsImpl.fstatSync(handle);
        const afterPath = inspect();
        if (
          !sameSnapshot(opened, afterOpen) ||
          !sameSnapshot(opened, afterPath)
        )
          throw new Acp05Error("marker-log", "leaf changed during read");
        return { content };
      } finally {
        fsImpl.closeSync(handle);
      }
    },
  };
}

function markerMatches(text, config) {
  if (!text.includes(ACP05_MARKER)) return false;
  const hasLauncherAttestation = text.includes("launcher-attestation:");
  if (config.markerProof === "native-pairing-marker" && hasLauncherAttestation)
    return false;
  if (config.markerProof === "launcher-attested-owned-stdout")
    return (
      text.trim() ===
      `launcher-attestation: ${ACP05_MARKER} nonce=${config.markerNonce} pid=${config.processId} source=native-stdout.raw.log`
    );
  if (config.markerNonce && !text.includes(`nonce=${config.markerNonce}`))
    return false;
  if (config.processId && !text.includes(`pid=${config.processId}`))
    return false;
  return true;
}

function exactMarkerLine(content, config) {
  let start = 0;
  while (start < content.length) {
    const newline = content.indexOf(0x0a, start);
    const end = newline === -1 ? content.length : newline + 1;
    const line = content.subarray(start, end);
    if (
      line.length <= LIMITS.maxMarkerBytes &&
      markerMatches(line.toString("utf8"), config)
    )
      return { content: Buffer.from(line), relativeOffset: start };
    if (newline === -1) break;
    start = end;
  }
  return null;
}

async function waitForMarker(source, offset, config, gate, sleepFn) {
  while (gate.remaining() > 0) {
    const update = source.readFrom(offset);
    const content = Buffer.isBuffer(update.content)
      ? update.content
      : Buffer.from(update.text ?? "", "utf8");
    if (content.length > LIMITS.maxLogBytes)
      throw new Acp05Error("marker-log", "oversized");
    const retained = exactMarkerLine(content, config);
    if (retained) {
      return {
        content: retained.content,
        record: {
          observed: true,
          afterOffset: offset + retained.relativeOffset,
          bytes: retained.content.length,
          sha256: hash(retained.content),
          nonceBound: Boolean(config.markerNonce),
          processBound: Boolean(config.processId),
          proof: config.markerProof,
        },
      };
    }
    await gate.within(
      sleepFn(Math.min(LIMITS.pollMs, gate.remaining())),
      "marker wait",
    );
  }
  throw new Acp05Error("marker-missing");
}

function decodeBase64(value) {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.length % 4 !== 0 ||
    !/^[A-Za-z0-9+/]*={0,2}$/u.test(value)
  )
    throw new Acp05Error("screenshot", "invalid base64");
  const padding = value.endsWith("==") ? 2 : value.endsWith("=") ? 1 : 0;
  const decodedBytes = (value.length / 4) * 3 - padding;
  if (decodedBytes < 1 || decodedBytes > LIMITS.maxScreenshotBytes)
    throw new Acp05Error("screenshot", "oversized");
  const decoded = Buffer.from(value, "base64");
  if (decoded.length !== decodedBytes)
    throw new Acp05Error("screenshot", "invalid base64");
  return decoded;
}

function writeOutput(fsImpl, root, target, content) {
  const resolved = assertPlainExistingParents(fsImpl, root, target);
  if (fsImpl.existsSync(resolved)) throw new Acp05Error("output-exists");
  fsImpl.writeFileSync(resolved, content, { flag: "wx" });
  return {
    name: path.relative(root, resolved).split(path.sep).join("/"),
    bytes: Buffer.byteLength(content),
    sha256: hash(content),
  };
}

export function buildReceipt(config, fields) {
  const launcherAttested =
    config.markerProof === "launcher-attested-owned-stdout";
  return {
    schemaVersion: SCHEMA_VERSION,
    kind: KIND,
    outcome: fields.outcome,
    evidenceStatus: launcherAttested
      ? "collection-only-incomplete"
      : "native-complete",
    source: {
      commit: config.sourceCommit,
      executable: {
        sha256: config.executableSha256,
        bytes: config.executableBytes,
        binding: "collection-only",
      },
    },
    cdp: fields.cdp ?? null,
    marker: fields.marker ?? null,
    before: fields.before ?? null,
    after: fields.after ?? null,
    artifacts: fields.artifacts ?? [],
    error: fields.error ?? null,
  };
}

export function validateCollectionReceipt(receipt) {
  if (
    !exactKeys(receipt, [
      "schemaVersion",
      "kind",
      "outcome",
      "evidenceStatus",
      "source",
      "cdp",
      "marker",
      "before",
      "after",
      "artifacts",
      "error",
    ]) ||
    receipt.schemaVersion !== SCHEMA_VERSION ||
    receipt.kind !== KIND ||
    !["passed", "collected"].includes(receipt.outcome) ||
    receipt.error !== null ||
    !exactKeys(receipt.source, ["commit", "executable"]) ||
    !HEX40.test(receipt.source.commit) ||
    !exactKeys(receipt.source.executable, ["sha256", "bytes", "binding"]) ||
    !HEX64.test(receipt.source.executable.sha256) ||
    !Number.isSafeInteger(receipt.source.executable.bytes) ||
    receipt.source.executable.bytes < 1 ||
    receipt.source.executable.binding !== "collection-only" ||
    !receipt.cdp ||
    !receipt.marker?.observed ||
    !receipt.before ||
    !receipt.after ||
    !Array.isArray(receipt.artifacts) ||
    receipt.artifacts.length !== 4
  ) {
    throw new Acp05Error("receipt-invalid");
  }
  validatePacket(receipt.before.packet, "before");
  validatePacket(receipt.after.packet, "after");
  for (const record of [receipt.before, receipt.after]) {
    if (
      !HEX64.test(record.packetSha256) ||
      !HEX64.test(record.loaderSha256) ||
      hash(Buffer.from(JSON.stringify(record.packet))) !== record.packetSha256
    )
      throw new Acp05Error("receipt-invalid", "packet hash");
  }
  if (receipt.before.loaderSha256 === receipt.after.loaderSha256)
    throw new Acp05Error("receipt-invalid", "loader did not change");
  for (const artifact of receipt.artifacts) {
    if (
      !exactKeys(artifact, ["name", "bytes", "sha256"]) ||
      !/^[a-z0-9.-]+$/u.test(artifact.name) ||
      !Number.isSafeInteger(artifact.bytes) ||
      artifact.bytes < 1 ||
      !HEX64.test(artifact.sha256)
    )
      throw new Acp05Error("receipt-invalid", "artifact");
  }
  const retainedMarker = receipt.artifacts.find(
    (artifact) => artifact.name === receipt.marker.artifact,
  );
  if (
    !MARKER_PROOFS.has(receipt.marker.proof) ||
    receipt.evidenceStatus !==
      (receipt.marker.proof === "native-pairing-marker"
        ? "native-complete"
        : "collection-only-incomplete") ||
    receipt.outcome !==
      (receipt.marker.proof === "native-pairing-marker"
        ? "passed"
        : "collected") ||
    receipt.marker.artifact !==
      (receipt.marker.proof === "native-pairing-marker"
        ? "acp05-native-marker.log"
        : "acp05-launcher-attested-marker.log") ||
    !retainedMarker ||
    retainedMarker.sha256 !== receipt.marker.sha256 ||
    retainedMarker.bytes !== receipt.marker.bytes
  )
    throw new Acp05Error("receipt-invalid", "marker artifact");
  return true;
}

export function validateReceipt(receipt) {
  validateCollectionReceipt(receipt);
  if (
    receipt.evidenceStatus !== "native-complete" ||
    receipt.marker.proof !== "native-pairing-marker"
  )
    throw new Acp05Error("receipt-invalid", "native-marker-required");
  return true;
}

export function receiptExitCode(receipt) {
  if (receipt?.outcome === "passed") return 0;
  if (
    receipt?.outcome === "collected" &&
    receipt.evidenceStatus === "collection-only-incomplete" &&
    receipt.marker?.proof === "launcher-attested-owned-stdout"
  )
    return 0;
  return 1;
}

export async function runCapture(rawConfig, deps = {}) {
  const fsImpl = deps.fs ?? fs;
  const now = deps.now ?? (() => performance.now());
  const sleepFn = deps.sleep ?? sleep;
  const config = validateConfig(rawConfig, fsImpl);
  const gate = deadlineGate(now, config.deadlineMs);
  const markerSource =
    deps.markerSource ?? fileMarkerSource(config.markerLog, fsImpl);
  let transport;
  let cdp = null;
  let marker = null;
  let before = null;
  let after = null;
  let artifacts = [];
  let error = null;
  try {
    const target = await discoverTarget(
      config,
      deps.fetch ?? globalThis.fetch,
      gate,
      sleepFn,
    );
    cdp = {
      port: config.cdpPort,
      targetId: target.id,
      targetIdSha256: hash(target.id),
      origin: target.url,
    };
    transport = (deps.createTransport ?? createTransport)(
      target.webSocketDebuggerUrl,
      { WebSocketImpl: deps.WebSocket ?? globalThis.WebSocket },
    );
    await gate.within(
      new Promise((resolve, reject) => {
        transport.onOpen(resolve);
        transport.onError(reject);
      }),
      "websocket open",
    );
    const client = createClient(transport, gate);
    await client.call("Page.enable");
    const frame = await client.call("Page.getFrameTree");
    const previousLoaderId = frame?.frameTree?.frame?.loaderId;
    if (typeof previousLoaderId !== "string")
      throw new Acp05Error("reload-correlation", "missing initial loader");
    before = await capturePacket(client, "before", previousLoaderId);
    // Capture the offset before reload; only appended output can prove this run.
    const markerOffset = markerSource.start();
    await client.call("Page.reload");
    const nextLoaderId = await waitForReadyDocument(
      client,
      previousLoaderId,
      gate,
      sleepFn,
    );
    after = await capturePacket(client, "after", nextLoaderId);
    const retainedMarker = await waitForMarker(
      markerSource,
      markerOffset,
      config,
      gate,
      sleepFn,
    );
    const screenshot = decodeBase64(
      (await client.call("Page.captureScreenshot", { format: "png" }))?.data,
    );
    const markerArtifactName =
      config.markerProof === "native-pairing-marker"
        ? "acp05-native-marker.log"
        : "acp05-launcher-attested-marker.log";
    const markerArtifact = writeOutput(
      fsImpl,
      config.evidenceRoot,
      path.join(config.evidenceRoot, markerArtifactName),
      retainedMarker.content,
    );
    marker = { ...retainedMarker.record, artifact: markerArtifact.name };
    artifacts = [
      writeOutput(
        fsImpl,
        config.evidenceRoot,
        path.join(config.evidenceRoot, "acp05-before-packet.json"),
        Buffer.from(`${JSON.stringify(before.packet)}\n`),
      ),
      writeOutput(
        fsImpl,
        config.evidenceRoot,
        path.join(config.evidenceRoot, "acp05-after-packet.json"),
        Buffer.from(`${JSON.stringify(after.packet)}\n`),
      ),
      writeOutput(
        fsImpl,
        config.evidenceRoot,
        path.join(config.evidenceRoot, "acp05-after.png"),
        screenshot,
      ),
      markerArtifact,
    ];
    client.close();
    transport = null;
  } catch (caught) {
    error =
      caught instanceof Acp05Error
        ? caught.message
        : `acp05-collection: ${String(caught?.message ?? caught).slice(0, 256)}`;
  } finally {
    gate.close();
    transport?.close();
  }
  const receipt = buildReceipt(config, {
    outcome: error
      ? "failed"
      : config.markerProof === "native-pairing-marker"
        ? "passed"
        : "collected",
    cdp,
    marker,
    before,
    after,
    artifacts,
    error,
  });
  try {
    writeOutput(
      fsImpl,
      config.evidenceRoot,
      config.receipt,
      Buffer.from(`${JSON.stringify(receipt, null, 2)}\n`),
    );
  } catch (receiptError) {
    if (!error) throw receiptError;
  }
  return receipt;
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    const receipt = await runCapture(parseArgs(process.argv.slice(2)));
    process.stdout.write(`${JSON.stringify(receipt, null, 2)}\n`);
    process.exitCode = receiptExitCode(receipt);
  } catch (error) {
    process.stderr.write(`${String(error?.message ?? error)}\n`);
    process.exitCode = 64;
  }
}
