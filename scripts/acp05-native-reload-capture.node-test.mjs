import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  Acp05Error,
  ACP05_MARKER,
  RENDERER_RECREATION_PACKET_EVAL,
  assertWithinRoot,
  parseArgs,
  runCapture,
  validateConfig,
  validateCollectionReceipt,
  validatePacket,
  validateReceipt,
} from "./acp05-native-reload-capture.mjs";

const canonicalLivePacket = Object.freeze({
  schemaVersion: 1,
  liveRegions: { polite: 1, assertive: 1 },
  pagehideListeners: 1,
  focusOutcome: "exact-sage-id",
  projectionSlots: 2,
});

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "sage-acp05-"));
  const executablePath = path.join(root, "sage.exe");
  fs.writeFileSync(executablePath, "fixture executable");
  const markerLog = path.join(root, "sage.log");
  fs.writeFileSync(markerLog, "pre-existing log line\n");
  return {
    root,
    executablePath,
    markerLog,
    executableSha256: crypto
      .createHash("sha256")
      .update("fixture executable")
      .digest("hex"),
  };
}

function config(value, extra = {}) {
  return {
    repoRoot: value.root,
    sourceCommit: "a".repeat(40),
    executablePath: value.executablePath,
    executableSha256: value.executableSha256,
    cdpEndpoint: "http://127.0.0.1:9222",
    evidenceRoot: path.join(value.root, "local-evidence", "acp05"),
    markerLog: value.markerLog,
    deadlineMs: 1_000,
    ...extra,
  };
}

class FakeTransport {
  constructor(responder) {
    this.responder = responder;
    this.messages = [];
    this.opens = [];
    this.errors = [];
  }
  onMessage(callback) {
    this.messages.push(callback);
  }
  onOpen(callback) {
    this.opens.push(callback);
  }
  onError(callback) {
    this.errors.push(callback);
  }
  send(message) {
    const response = this.responder(message);
    if (response)
      queueMicrotask(() =>
        this.messages.forEach((callback) => callback(response)),
      );
  }
  open() {
    this.opens.forEach((callback) => callback());
  }
  close() {}
}

function captureDeps({
  before = canonicalLivePacket,
  after = canonicalLivePacket,
  marker = ACP05_MARKER,
  target = null,
} = {}) {
  let reloaded = false;
  let packetCalls = 0;
  const responder = ({ id, method, params }) => {
    if (method === "Page.enable") return { id, result: {} };
    if (method === "Page.reload") {
      reloaded = true;
      return { id, result: {} };
    }
    if (method === "Page.getFrameTree")
      return {
        id,
        result: {
          frameTree: {
            frame: { loaderId: reloaded ? "loader-after" : "loader-before" },
          },
        },
      };
    if (method === "Page.captureScreenshot")
      return { id, result: { data: Buffer.from("png").toString("base64") } };
    if (method === "Runtime.evaluate") {
      if (params.expression === RENDERER_RECREATION_PACKET_EVAL) {
        packetCalls += 1;
        return {
          id,
          result: { result: { value: packetCalls === 1 ? before : after } },
        };
      }
      return {
        id,
        result: {
          result: {
            value: {
              title: "Sage",
              href: "http://tauri.localhost/",
              readyState: "complete",
            },
          },
        },
      };
    }
    return { id, result: {} };
  };
  return {
    fetch: async () => ({
      ok: true,
      text: async () =>
        JSON.stringify([
          target ?? {
            id: "target-1",
            type: "page",
            title: "Sage",
            url: "http://tauri.localhost/",
            webSocketDebuggerUrl: "ws://127.0.0.1:9222/devtools/page/target-1",
          },
        ]),
    }),
    createTransport: () => {
      const transport = new FakeTransport(responder);
      queueMicrotask(() => transport.open());
      return transport;
    },
    markerSource: {
      source: "fixture",
      start: () => 19,
      readFrom: () => ({
        text: reloaded ? marker : "",
        bytes: reloaded ? Buffer.byteLength(marker) : 0,
      }),
    },
    sleep: () => new Promise((resolve) => setTimeout(resolve, 1)),
  };
}

test("argument parsing has one implementation and rejects duplicate flags", () => {
  const options = parseArgs([
    "--repo-root",
    "C:\\repo",
    "--marker-log",
    "C:\\log",
  ]);
  assert.equal(options.repoRoot, "C:\\repo");
  assert.throws(
    () => parseArgs(["--repo-root", "x", "--repo-root", "y"]),
    Acp05Error,
  );
});

test("config requires ignored local-evidence, a live log, and an exact loopback port", () => {
  const value = fixture();
  assert.throws(
    () =>
      validateConfig(
        config(value, { evidenceRoot: path.join(value.root, "elsewhere") }),
      ),
    /path-escape/u,
  );
  assert.throws(
    () => validateConfig(config(value, { cdpEndpoint: "http://127.0.0.1" })),
    /cdp-endpoint/u,
  );
  assert.throws(
    () =>
      validateConfig(
        config(value, { markerLog: path.join(value.root, "missing.log") }),
      ),
    /marker-log/u,
  );
  assert.equal(validateConfig(config(value)).cdpPort, 9222);
});

test("path containment rejects traversal", () => {
  assert.throws(
    () => assertWithinRoot("C:\\evidence", "C:\\outside"),
    Acp05Error,
  );
});

test("both samples require the canonical admitted live-page packet", () => {
  assert.throws(() => validatePacket(null, "before"), /packet-invalid/u);
  assert.throws(
    () =>
      validatePacket({ ...canonicalLivePacket, projectionSlots: 0 }, "before"),
    /packet-invalid/u,
  );
  assert.throws(
    () =>
      validatePacket(
        {
          ...canonicalLivePacket,
          liveRegions: { polite: 1, assertive: 0 },
        },
        "after",
      ),
    /packet-invalid/u,
  );
});

test("collection binds a new loader, post-offset marker, artifact hashes, and requested receipt", async () => {
  const value = fixture();
  const receiptPath = path.join(
    value.root,
    "local-evidence",
    "acp05",
    "nested",
    "receipt.json",
  );
  const receipt = await runCapture(
    config(value, { receipt: receiptPath }),
    captureDeps(),
  );
  assert.equal(receipt.outcome, "passed");
  assert.equal(receipt.source.executable.binding, "collection-only");
  assert.equal(receipt.marker.afterOffset, 19);
  assert.equal(receipt.marker.artifact, "acp05-native-marker.log");
  assert.notEqual(receipt.before.loaderSha256, receipt.after.loaderSha256);
  assert.equal(receipt.artifacts.length, 4);
  validateReceipt(receipt);
  const serialized = fs.readFileSync(receiptPath, "utf8");
  assert.equal(
    serialized.includes(value.root),
    false,
    "receipt must not leak host paths",
  );
  for (const artifact of receipt.artifacts) {
    const file = path.join(path.dirname(receiptPath), "..", artifact.name);
    assert.equal(
      artifact.sha256,
      crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex"),
    );
  }
});

test("receipt validation requires the retained marker artifact and its hash binding", async () => {
  const value = fixture();
  const receipt = await runCapture(config(value), captureDeps());
  const markerArtifact = receipt.artifacts.find(
    (artifact) => artifact.name === "acp05-native-marker.log",
  );
  assert.ok(markerArtifact);
  assert.equal(markerArtifact.sha256, receipt.marker.sha256);
  assert.equal(markerArtifact.bytes, receipt.marker.bytes);
  assert.throws(
    () =>
      validateReceipt({
        ...receipt,
        artifacts: receipt.artifacts.filter(
          (artifact) => artifact.name !== "acp05-native-marker.log",
        ),
      }),
    /receipt-invalid/u,
  );
  assert.throws(
    () =>
      validateReceipt({
        ...receipt,
        after: {
          ...receipt.after,
          loaderSha256: receipt.before.loaderSha256,
        },
      }),
    /receipt-invalid/u,
  );
});

test("marker log symlinks are rejected at the config seam", () => {
  const value = fixture();
  const realLstat = fs.lstatSync.bind(fs);
  const fsWithLinkedLog = {
    ...fs,
    lstatSync(candidate) {
      const stat = realLstat(candidate);
      if (path.resolve(candidate) !== path.resolve(value.markerLog))
        return stat;
      return new Proxy(stat, {
        get(target, property) {
          if (property === "isSymbolicLink") return () => true;
          return Reflect.get(target, property, target);
        },
      });
    },
  };
  assert.throws(
    () => validateConfig(config(value), fsWithLinkedLog),
    /marker-log/u,
  );
});

test("collection fails closed for malformed before, mismatched after, and escaped websocket", async () => {
  const value = fixture();
  const cases = [
    [captureDeps({ before: null }), /packet-invalid/u],
    [
      captureDeps({
        after: { ...canonicalLivePacket, pagehideListeners: 0 },
      }),
      /packet-invalid/u,
    ],
    [
      captureDeps({
        target: {
          id: "target-1",
          type: "page",
          title: "Sage",
          url: "http://tauri.localhost/",
          webSocketDebuggerUrl: "ws://127.0.0.1:9333/devtools/page/target-1",
        },
      }),
      /cdp-target/u,
    ],
  ];
  for (const [deps, expected] of cases) {
    const receipt = await runCapture(
      config(value, {
        evidenceRoot: path.join(
          value.root,
          "local-evidence",
          `case-${Math.random()}`,
        ),
      }),
      deps,
    );
    assert.equal(receipt.outcome, "failed");
    assert.match(receipt.error, expected);
  }
});

test("a marker already present before the captured log offset is never accepted", async () => {
  const value = fixture();
  fs.writeFileSync(value.markerLog, `${ACP05_MARKER}\n`);
  const deps = captureDeps();
  delete deps.markerSource;
  const receipt = await runCapture(config(value), deps);
  assert.equal(receipt.outcome, "failed");
  assert.match(receipt.error, /marker-missing|deadline/u);
});

test("marker nonce and process binding reject an unbound new marker", async () => {
  const value = fixture();
  const receipt = await runCapture(
    config(value, { markerNonce: "nonce-1234", processId: "42" }),
    captureDeps(),
  );
  assert.equal(receipt.outcome, "failed");
  assert.match(receipt.error, /marker-missing|deadline/u);
});

test("launcher-attested marker is collection-only incomplete and cannot validate as native", async () => {
  const value = fixture();
  const nonce = "nonce-1234";
  const processId = "42";
  const marker = `launcher-attestation: ${ACP05_MARKER} nonce=${nonce} pid=${processId} source=native-stdout.raw.log\n`;
  const receipt = await runCapture(
    config(value, {
      markerNonce: nonce,
      markerProof: "launcher-attested-owned-stdout",
      processId,
    }),
    captureDeps({ marker }),
  );
  assert.equal(receipt.outcome, "collected");
  assert.equal(receipt.evidenceStatus, "collection-only-incomplete");
  assert.equal(receipt.marker.proof, "launcher-attested-owned-stdout");
  assert.equal(receipt.marker.artifact, "acp05-launcher-attested-marker.log");
  assert.equal(validateCollectionReceipt(receipt), true);
  assert.throws(() => validateReceipt(receipt), /native-marker-required/u);
});

test("native proof classification rejects launcher-attested text", async () => {
  const value = fixture();
  const receipt = await runCapture(
    config(value),
    captureDeps({
      marker: `launcher-attestation: ${ACP05_MARKER} source=native-stdout.raw.log\n`,
    }),
  );
  assert.equal(receipt.outcome, "failed");
  assert.match(receipt.error, /marker-missing|deadline/u);
});

test("collector subprocess exits zero for truthful collected evidence while native validation rejects it", async () => {
  const value = fixture();
  const nonce = "nonce-1234";
  const processId = "42";
  const receipt = await runCapture(
    config(value, {
      markerNonce: nonce,
      markerProof: "launcher-attested-owned-stdout",
      processId,
    }),
    captureDeps({
      marker: `launcher-attestation: ${ACP05_MARKER} nonce=${nonce} pid=${processId} source=native-stdout.raw.log\n`,
    }),
  );
  const moduleUrl = new URL(
    "./acp05-native-reload-capture.mjs",
    import.meta.url,
  ).href;
  const script = `import { receiptExitCode, validateCollectionReceipt, validateReceipt } from ${JSON.stringify(moduleUrl)};
const receipt=JSON.parse(process.env.ACP05_RECEIPT);
validateCollectionReceipt(receipt);
let nativeRejected=false;
try { validateReceipt(receipt); } catch (error) { nativeRejected=/native-marker-required/u.test(String(error?.message)); }
if (!nativeRejected) process.exit(70);
process.exit(receiptExitCode(receipt));`;
  const child = spawnSync(
    process.execPath,
    ["--input-type=module", "--eval", script],
    {
      encoding: "utf8",
      env: { ...process.env, ACP05_RECEIPT: JSON.stringify(receipt) },
    },
  );
  assert.equal(child.status, 0, child.stderr);
});
