import {
  closeSync,
  lstatSync,
  existsSync,
  openSync,
  readSync,
  writeFileSync,
  unlinkSync,
} from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";
import { MAX_BURST_FRAMES } from "./capture-native-chat-motion.mjs";
import {
  parseArgs,
  evidencePaths,
  requireNativeCase,
  validateProbe,
  validateDimensions,
  validatePng,
  assertPrivacy,
} from "./capture-native-chat-contract.mjs";
import {
  requirePlain,
  ensurePlainDirectory,
} from "./capture-native-chat-process.mjs";

const RUN_ID =
  /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/u;
export function safeDestination(file) {
  try {
    if (!lstatSync(file).isFile() || lstatSync(file).isSymbolicLink())
      throw new Error("evidence-reparse");
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
}
/**
 * A motion burst beside the still, rec.ps1-style: `<stem>-fNN-<ms>ms.png`
 * frames plus `<stem>.frames.json` (index, wall and animation offsets,
 * bytes, SHA-256). Every frame is a validated viewport PNG in the same
 * staging directory.
 */
export function burstManifest(png, options, burst) {
  if (
    !burst ||
    !["wall", "seek"].includes(burst.mode) ||
    !Array.isArray(burst.frames) ||
    burst.frames.length < 2 ||
    burst.frames.length > MAX_BURST_FRAMES
  )
    throw new Error("burst-manifest");
  const base = path.dirname(png),
    stem = path.basename(png, ".png");
  const files = burst.frames.map((frame, index) => {
    const offset = frame.animationMs ?? frame.atMs;
    if (
      frame.index !== index ||
      !Number.isSafeInteger(frame.atMs) ||
      frame.atMs < 0 ||
      !Number.isSafeInteger(offset) ||
      offset < 0 ||
      offset > 600000
    )
      throw new Error("burst-manifest");
    validatePng(frame.png, options);
    const name =
      stem +
      "-f" +
      String(index + 1).padStart(2, "0") +
      "-" +
      String(offset).padStart(6, "0") +
      "ms.png";
    const file = path.resolve(base, name);
    if (path.dirname(file) !== base) throw new Error("evidence-path");
    safeDestination(file);
    return {
      file,
      png: frame.png,
      entry: {
        name,
        index,
        atMs: frame.atMs,
        animationMs: frame.animationMs,
        bytes: frame.png.length,
        sha256: createHash("sha256").update(frame.png).digest("hex"),
      },
    };
  });
  const manifest = {
    schemaVersion: 1,
    caseId: options.caseId,
    width: options.width,
    mode: burst.mode,
    intervalMs: burst.intervalMs,
    frames: files.map(({ entry }) => entry),
  };
  assertPrivacy(manifest);
  const target = path.resolve(base, stem + ".frames.json");
  safeDestination(target);
  return {
    files,
    path: target,
    json: JSON.stringify(manifest, null, 2) + "\n",
  };
}
export function persistCapture(
  {
    output,
    options,
    run,
    directory,
    timing,
    elapsedMs,
    nativeBounds,
    captured,
    error,
  },
  write = writeFileSync,
) {
  const created = [];
  const createFile = (file, bytes) => {
    const handle = openSync(file, "wx");
    created.push(file);
    try {
      write(handle, bytes);
    } finally {
      closeSync(handle);
    }
  };
  if (!error) {
    try {
      captured.probe.timing = { elapsedMs, steps: timing.steps };
      validateProbe(captured.probe);
      createFile(output.png, captured.screenshot);
      const burst = captured.burst
        ? burstManifest(output.png, options, captured.burst)
        : null;
      for (const frame of burst?.files ?? []) createFile(frame.file, frame.png);
      if (burst) createFile(burst.path, burst.json);
      write(output.probe, JSON.stringify(captured.probe, null, 2) + "\n");
      return {
        verdict: "captured",
        run,
        directory,
        png: path.basename(output.png),
        ...(burst ? { frames: burst.files.length } : {}),
      };
    } catch (failure) {
      error = failure;
      if (created.length) {
        if (timing.steps.length < 64) timing.record("png-rollback-start");
        try {
          for (const file of created.reverse()) unlinkSync(file);
          if (timing.steps.length < 64) timing.record("png-rollback-finished");
        } catch (cleanupError) {
          error = new Error("capture-png-cleanup", { cause: cleanupError });
        }
      }
    }
  }
  const reason = /^[a-z0-9-]{1,80}$/u.test(error.message)
    ? error.message
    : "capture-operation-blocked";
  if (timing.steps.length < 64) timing.record("capture-blocked");
  const blocked = {
    schemaVersion: 1,
    caseId: options.caseId,
    harness: options.harness,
    state: options.state,
    width: options.width,
    verdict: "blocked",
    reason,
    timing: { elapsedMs, steps: timing.steps },
  };
  if (nativeBounds) blocked.nativeBounds = nativeBounds;
  if (error.cleanupError) blocked.cleanupError = error.cleanupError;
  if (error.appExit) blocked.appExit = error.appExit;
  assertPrivacy(blocked);
  write(output.probe, JSON.stringify(blocked, null, 2) + "\n");
  return { ...blocked, run };
}
export function stagingPaths(workspace, run, options) {
  if (!RUN_ID.test(run)) throw new Error("capture-run");
  requireNativeCase(options.harness, options.state);
  if (options.caseId !== options.harness + "-" + options.state)
    throw new Error("case-identity");
  const directory = path.resolve(workspace, "work", "native-captures", run);
  return { directory, ...evidencePaths(directory, options) };
}
export function parsePromotionArgs(args) {
  if (args.length !== 9 || args[0] !== "--run" || args.at(-1) !== "--reviewed")
    throw new Error("review-required");
  const options = {
    ...parseArgs(args.slice(2, -1)),
    run: args[1],
    reviewed: true,
  };
  if (!RUN_ID.test(options.run)) throw new Error("capture-run");
  return options;
}
function readStagedFile(file, maximum) {
  const handle = openSync(requirePlain(file), "r");
  const bytes = Buffer.alloc(maximum + 1);
  let length = 0;
  try {
    for (;;) {
      const count = readSync(
        handle,
        bytes,
        length,
        bytes.length - length,
        null,
      );
      if (count === 0) return bytes.subarray(0, length);
      length += count;
      if (length > maximum) throw new Error("capture-file-bound");
    }
  } finally {
    closeSync(handle);
  }
}
export function promoteCapture(workspace, options) {
  if (options.reviewed !== true) throw new Error("review-required");
  requirePlain(workspace, true);
  const staged = stagingPaths(workspace, options.run, options);
  const probeBytes = readStagedFile(staged.probe, 65536);
  const probeText = new TextDecoder("utf-8", { fatal: true }).decode(
    probeBytes,
  );
  assertPrivacy(probeText);
  const probe = validateProbe(JSON.parse(probeText));
  const probeOutput = Buffer.from(JSON.stringify(probe, null, 2) + "\n");
  if (probeOutput.length > 65536) throw new Error("probe-bytes");
  if (probe.caseId !== options.caseId) throw new Error("case-identity");
  validateDimensions(
    probe.dimensions,
    options.width,
    options.width === 1888 ? 1096 : 1000,
    probe.dpr,
  );
  const png = readStagedFile(staged.png, 8 * 1024 * 1024);
  validatePng(png, options);
  const directory = path.join(
    workspace,
    "docs",
    "work-items",
    "CODEX-PARITY",
    "evidence",
  );
  ensurePlainDirectory(directory);
  const output = evidencePaths(directory, options);
  if (Object.values(output).some((file) => existsSync(file)))
    throw new Error("evidence-exists");
  writeFileSync(output.png, png, { flag: "wx" });
  try {
    writeFileSync(output.probe, probeOutput, {
      flag: "wx",
    });
  } catch (error) {
    unlinkSync(output.png);
    throw error;
  }
  return {
    verdict: "promoted",
    run: options.run,
    png: path.basename(output.png),
    probe: path.basename(output.probe),
  };
}
