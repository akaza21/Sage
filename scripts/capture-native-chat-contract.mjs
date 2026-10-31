import path from "node:path";
import {
  IDENTIFIER,
  retainCaptureError,
} from "./capture-native-chat-shared.mjs";
import { HARNESS } from "./capture-native-chat-dom.mjs";
import {
  nativeCases,
  truthfulAbsentStates,
  nativeCaptureProductGaps,
} from "./acp-native-cases.mjs";
import { privacyFailuresForRecords } from "./check-harness-trace-privacy.mjs";
import { parityWidths } from "./capture-native-chat-parity.mjs";

export const STATES = new Set([
  ...nativeCases().map(({ id }) => id.replace(/^(codex|claude|pi)-/u, "")),
  ...Object.values(truthfulAbsentStates).flat(),
  ...Object.values(nativeCaptureProductGaps).flat(),
]);
const CASE_IDS = new Set(nativeCases().map(({ id }) => id));
export function requireNativeCase(harness, state) {
  if (truthfulAbsentStates[harness]?.includes(state))
    fail("truthful-absent-state");
  if (nativeCaptureProductGaps[harness]?.includes(state))
    fail("product-gap-state");
  if (!CASE_IDS.has(harness + "-" + state)) fail("unsupported-state");
}
const SHA = /^[a-f0-9]{64}$/u;
function fail(code) {
  throw new Error(code);
}
function keys(value, allowed) {
  if (
    !value ||
    typeof value !== "object" ||
    Array.isArray(value) ||
    Object.keys(value).sort().join("|") !== [...allowed].sort().join("|")
  )
    fail("probe-schema");
}
function finite(value, min = 0, max = 16384) {
  if (!Number.isFinite(value) || value < min || value > max)
    fail("probe-number");
}
function flags(value) {
  if (
    !value ||
    typeof value !== "object" ||
    Array.isArray(value) ||
    Object.keys(value).length > 32 ||
    Object.entries(value).some(
      ([key, value]) => !IDENTIFIER.test(key) || typeof value !== "boolean",
    )
  )
    fail("probe-flags");
}
export function assertPrivacy(value) {
  const json = JSON.stringify(value);
  if (
    privacyFailuresForRecords([value], false).length ||
    /(?:sk-(?:ant-)?|Bearer\s|api[_-]?key|secret-command|private-sentinel)/iu.test(
      json,
    )
  )
    fail("privacy");
}
/**
 * Base states whose witness the product hides at a capture width. The composer
 * drops its context meter in containers up to 280 px wide
 * (chat-container-controls.css), and the 480 window's chat column is one.
 */
const BASE_STATE_WIDTHS = Object.freeze({ "context-meter": [1888, 720] });
export function captureWidths(state) {
  return BASE_STATE_WIDTHS[state] ?? parityWidths(state);
}
export function parseArgs(args) {
  const values = {};
  if (args.length !== 6) fail("arguments");
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    if (!["--harness", "--state", "--width"].includes(key) || key in values)
      fail("arguments");
    values[key] = args[index + 1];
  }
  const harness = values["--harness"],
    state = values["--state"],
    width = Number(values["--width"]);
  if (
    !["codex", "claude", "pi"].includes(harness) ||
    !STATES.has(state) ||
    !["1888", "720", "480"].includes(values["--width"])
  )
    fail("arguments");
  requireNativeCase(harness, state);
  const responsiveWidth = {
    "responsive-wide": 1888,
    "responsive-720": 720,
    "responsive-480": 480,
  }[state];
  if (responsiveWidth && responsiveWidth !== width)
    fail("responsive-width-mismatch");
  // A surface the product hides below a pane width is not captured there.
  if (!captureWidths(state).includes(width)) fail("state-width-unsupported");
  return {
    harness,
    state,
    width,
    height: width === 1888 ? 1096 : 1000,
    caseId: harness + "-" + state,
  };
}
export function evidencePaths(directory, options, admit = null) {
  if (admit) admit(options);
  else if (
    !STATES.has(options.state) ||
    !["codex", "claude", "pi"].includes(options.harness) ||
    ![1888, 720, 480].includes(options.width)
  )
    fail("evidence-path");
  const base = path.resolve(directory),
    stem = options.state + "-" + options.harness + "-" + options.width;
  const png = path.resolve(base, stem + ".png"),
    probe = path.resolve(base, stem + ".probe.json");
  if ([png, probe].some((file) => path.dirname(file) !== base))
    fail("evidence-path");
  return { png, probe };
}
export function validateDimensions(dimensions, width, height, dpr) {
  keys(dimensions, ["outer", "client", "css"]);
  for (const size of Object.values(dimensions)) {
    keys(size, ["width", "height"]);
    finite(size.width, 1);
    finite(size.height, 1);
  }
  if (dpr !== 1) fail("dpr-mismatch");
  if (
    ![1888, 720, 480].includes(width) ||
    height !== (width === 1888 ? 1096 : 1000) ||
    dimensions.client.width !== width ||
    dimensions.client.height !== height
  )
    fail("resize-clamp");
  if (
    dimensions.client.width !== dimensions.css.width ||
    dimensions.client.height !== dimensions.css.height ||
    dimensions.client.width > dimensions.outer.width ||
    dimensions.client.height > dimensions.outer.height
  )
    fail("viewport-mismatch");
}
export function validateProbe(probe, identity = validateChatIdentity) {
  keys(probe, [
    "schemaVersion",
    "caseId",
    "bindingId",
    "turnId",
    "hashes",
    "protocol",
    "webviewVersion",
    "dpr",
    "dimensions",
    "predicates",
    "assistantRendered",
    "lazyFallbackVisible",
    "elements",
    "controls",
    "overflow",
    "timing",
    "verdict",
  ]);
  if (probe.schemaVersion !== 1 || probe.verdict !== "captured")
    fail("probe-identity");
  identity(probe);
  validateProbeBody(probe);
  return probe;
}
function validateChatIdentity(probe) {
  if (
    probe.schemaVersion !== 1 ||
    probe.verdict !== "captured" ||
    !/^(codex|claude|pi)-[a-z0-9-]+$/u.test(probe.caseId) ||
    !CASE_IDS.has(probe.caseId) ||
    !["codex-app-server", "claude-agent-acp", "pi-rpc"].includes(
      probe.bindingId,
    ) ||
    !IDENTIFIER.test(probe.turnId) ||
    !["standard-acp-1", "codex-app-server-0.146.0", "pi-rpc-0.83.0"].includes(
      probe.protocol,
    )
  )
    fail("probe-identity");
  const harness = HARNESS[probe.caseId.split("-")[0]];
  if (
    probe.bindingId !== harness?.binding ||
    probe.protocol !== harness?.protocol
  )
    fail("probe-identity");
}
function validateProbeBody(probe) {
  if (
    typeof probe.webviewVersion !== "string" ||
    !/^\d+(?:\.\d+){1,3}$/u.test(probe.webviewVersion)
  )
    fail("probe-identity");
  keys(probe.hashes, ["build", "fixture", "source", "servedAsset"]);
  if (
    !SHA.test(probe.hashes.build) ||
    !SHA.test(probe.hashes.fixture) ||
    !SHA.test(probe.hashes.servedAsset) ||
    !/^[a-f0-9]{40,64}$/u.test(probe.hashes.source)
  )
    fail("probe-hash");
  validateDimensions(
    probe.dimensions,
    probe.dimensions.client.width,
    probe.dimensions.client.width === 1888 ? 1096 : 1000,
    probe.dpr,
  );
  flags(probe.predicates);
  flags(probe.controls);
  flags(probe.overflow);
  if (
    typeof probe.assistantRendered !== "boolean" ||
    typeof probe.lazyFallbackVisible !== "boolean"
  )
    fail("probe-rendered");
  if (probe.lazyFallbackVisible) fail("state-predicate");
  if (probe.predicates.namedState !== true || probe.predicates.binding !== true)
    fail("state-predicate");
  keys(probe.timing, ["elapsedMs", "steps"]);
  finite(probe.timing.elapsedMs, 0, 120000);
  if (!Array.isArray(probe.timing.steps) || probe.timing.steps.length > 64)
    fail("timing-bound");
  for (const step of probe.timing.steps) {
    keys(step, [
      "step",
      "atMs",
      "turnId",
      "state",
      "cancelPresent",
      "assistantRendered",
      "lazyFallbackVisible",
    ]);
    if (
      !IDENTIFIER.test(step.step) ||
      !IDENTIFIER.test(step.turnId) ||
      !IDENTIFIER.test(step.state) ||
      typeof step.cancelPresent !== "boolean" ||
      typeof step.assistantRendered !== "boolean" ||
      typeof step.lazyFallbackVisible !== "boolean"
    )
      fail("probe-timing");
    finite(step.atMs, 0, Number.MAX_SAFE_INTEGER);
  }
  if (!Array.isArray(probe.elements) || probe.elements.length > 64)
    fail("probe-elements");
  for (const element of probe.elements) {
    keys(element, ["kind", "text", "x", "y", "width", "height"]);
    if (
      !IDENTIFIER.test(element.kind) ||
      typeof element.text !== "string" ||
      Buffer.byteLength(element.text, "utf8") > 512
    )
      fail("probe-text");
    for (const field of ["x", "y"]) finite(element[field], -16384);
    for (const field of ["width", "height"]) finite(element[field]);
  }
  assertPrivacy(probe);
  if (Buffer.byteLength(JSON.stringify(probe), "utf8") > 65536)
    fail("probe-bytes");
  return probe;
}
/** Returns the single owned Sage page target, or null while none is present yet. */
export function selectOwnedTarget(
  targets,
  port,
  pageUrl = "http://127.0.0.1:1420/",
) {
  if (!Array.isArray(targets) || targets.length > 32) fail("cdp-target");
  const owned = targets.filter(
    (target) =>
      target?.type === "page" &&
      target.url === pageUrl &&
      /^Sage(?:$|\s)/u.test(target.title),
  );
  if (owned.length === 0) return null;
  if (owned.length !== 1) fail("cdp-target");
  const target = owned[0];
  if (
    !IDENTIFIER.test(target.id) ||
    target.webSocketDebuggerUrl !==
      "ws://127.0.0.1:" +
        port +
        "/devtools/page/" +
        encodeURIComponent(target.id)
  )
    fail("cdp-target");
  return target;
}
/**
 * Polls until exactly one owned page target appears. An unreachable or empty
 * listing waits; ambiguous or malformed owned targets and the deadline fail closed.
 */
export async function discoverOwnedTarget(poll, port, deadline, pageUrl) {
  let selected = null;
  try {
    await waitForState(async () => {
      const targets = await poll();
      if (targets === undefined) return false;
      selected = selectOwnedTarget(targets, port, pageUrl);
      return selected !== null;
    }, deadline);
  } catch (error) {
    if (error.message === "state-timeout") fail("cdp-target");
    throw error;
  }
  return selected;
}
/** Polls the owned app's loopback /json listing while the app stays alive. */
export function discoverOwnedPage(port, deadline, child, pageUrl) {
  return discoverOwnedTarget(
    async () => {
      if (child.exitCode !== null || child.signalCode !== null || !child.pid)
        fail("owned-app-exited");
      let response;
      try {
        response = await fetch("http://127.0.0.1:" + port + "/json", {
          signal: AbortSignal.timeout(1000),
        });
      } catch {
        return undefined;
      }
      const bytes = await readBoundedResponse(response);
      if (!response.ok) fail("cdp-discovery-bound");
      return JSON.parse(bytes);
    },
    port,
    deadline,
    pageUrl,
  );
}
export function validatePng(bytes, viewport) {
  if (
    !Buffer.isBuffer(bytes) ||
    bytes.length < 24 ||
    bytes.length > 8 * 1024 * 1024 ||
    bytes.subarray(0, 8).toString("hex") !== "89504e470d0a1a0a" ||
    bytes.toString("ascii", 12, 16) !== "IHDR" ||
    bytes.readUInt32BE(16) !== viewport.width ||
    bytes.readUInt32BE(20) !== viewport.height
  )
    fail("png-viewport");
  if (
    /(?:sk-ant-|secret-command|C:\\Users\\|\/home\/|\/Users\/)/u.test(
      bytes.toString("latin1"),
    )
  )
    fail("png-privacy");
}
export async function waitForState(predicate, deadline) {
  while (Date.now() < deadline) {
    if (await predicate()) return;
    await new Promise((resolve) =>
      setTimeout(resolve, Math.min(50, Math.max(0, deadline - Date.now()))),
    );
  }
  fail("state-timeout");
}
export async function withOwnedCapture(run, close, stop) {
  let result, failure;
  try {
    result = await run();
  } catch (error) {
    failure = error;
  }
  for (const cleanup of [close, stop]) {
    try {
      await cleanup();
    } catch (error) {
      failure = retainCaptureError(failure, error);
    }
  }
  if (failure) throw failure;
  return result;
}

export async function readBoundedBytes(response, maximum = 65536) {
  if (!response.body) fail("cdp-discovery-body");
  const reader = response.body.getReader(),
    chunks = [];
  let bytes = 0;
  try {
    for (;;) {
      const next = await reader.read();
      if (next.done) break;
      bytes += next.value.byteLength;
      if (bytes > maximum) {
        await reader.cancel();
        fail("cdp-discovery-bound");
      }
      chunks.push(Buffer.from(next.value));
    }
  } finally {
    reader.releaseLock();
  }
  return Buffer.concat(chunks, bytes);
}
export async function readBoundedResponse(response, maximum = 65536) {
  return new TextDecoder("utf-8", { fatal: true }).decode(
    await readBoundedBytes(response, maximum),
  );
}
