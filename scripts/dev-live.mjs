#!/usr/bin/env node
// `npm run dev`: the real UI against the real debug backend.
//
//   npm run dev [-- --port <n>] [-- --jobs <n>]
//
// Starts Vite on a free loopback port (or --port / SAGE_UI_PORT), then
// `tauri dev --features browser-bridge` with a generated config that points
// at that Vite and runs no beforeDevCommand. The backend uses a throwaway
// profile kept outside the checkout (devStateDirectory), never the real Sage
// profile, and publishes each run there; the dev server hands it to the tab, which pairs by
// itself and reloads after a Rust rebuild. Prints one URL. Both children run
// at below-normal priority and stop together, with everything they started.
// Cargo builds into this checkout's own target directory (or an explicit
// CARGO_TARGET_DIR), so a gate holding a shared target never stalls a rebuild.
// One launcher per checkout. Not on macOS: see refuseUnsupportedPlatform.
import { createHash } from "node:crypto";
import {
  closeSync,
  existsSync,
  mkdirSync,
  openSync,
  readSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { acquireLauncherLock } from "./lib/dev-live-lock.mjs";
import { killTrees, launch, signalRoots } from "./lib/dev-live-processes.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const STOP_GRACE_MS = 5_000;
const READY_TIMEOUT_MS = 60_000;
// Same bound as the dev server's route (scripts/devBridgeRun.ts).
const MAX_RECORD_BYTES = 512;

// WKWebView ignores Tauri's per-webview data directory, so on macOS the debug
// app's main window and in-app Preview would share WebKit website data
// (cookies, storage, Preview sign-ins) with the default store instead of the
// throwaway profile. Sage's launch profile refuses that (launch_profile.rs).
function refuseUnsupportedPlatform() {
  if (process.platform !== "darwin") return;
  throw new Error(
    "npm run dev is not available on macOS yet: WebKit has no per-profile " +
      "data directory, so the debug app could share browser data with your " +
      "real Sage. Use npm run tauri dev for a native debug window on your own profile.",
  );
}
const PROFILE_MARKER = "sage-disposable-profile-v1\n";

function flag(name) {
  const args = process.argv.slice(2);
  for (let index = 0; index < args.length; index++) {
    if (args[index] === `--${name}`) return args[index + 1];
    if (args[index].startsWith(`--${name}=`))
      return args[index].slice(name.length + 3);
  }
  return undefined;
}

function parsePort(value, source) {
  const port = Number(value);
  if (!/^\d{1,5}$/.test(value) || port < 1024 || port > 65535)
    throw new Error(`${source} must be a port between 1024 and 65535`);
  return port;
}

function listenOnce(port) {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen({ port, host: "127.0.0.1", exclusive: true }, () => {
      const { port: bound } = server.address();
      server.close(() => resolve(bound));
    });
  });
}

async function choosePort() {
  const requested = flag("port");
  if (requested !== undefined) return parsePort(requested, "--port");
  if (process.env.SAGE_UI_PORT !== undefined)
    return parsePort(process.env.SAGE_UI_PORT, "SAGE_UI_PORT");
  return listenOnce(0);
}

/**
 * Per-checkout state (profile, launcher lock, generated config) lives outside
 * the checkout, which is the dev server's root, so no request to the dev
 * server can ever reach the pairing code or the profile's database.
 */
function devStateDirectory() {
  const base =
    process.platform === "win32" && process.env.LOCALAPPDATA
      ? process.env.LOCALAPPDATA
      : os.tmpdir();
  const checkout = process.platform === "win32" ? root.toLowerCase() : root;
  const key = createHash("sha256").update(checkout).digest("hex").slice(0, 16);
  return path.join(base, "sage-dev", `${path.basename(root)}-${key}`);
}

function prepareProfile(devRoot) {
  const profile = path.join(devRoot, "profile");
  const durable = path.join(profile, "durable");
  mkdirSync(durable, { recursive: true });
  writeFileSync(
    path.join(profile, ".sage-disposable-profile-v1"),
    PROFILE_MARKER,
  );
  const runFile = path.join(durable, "dev-bridge-run.json");
  // A run from an earlier session would pair the tab with a dead backend.
  rmSync(runFile, { force: true });
  rmSync(`${runFile}.tmp`, { force: true });
  return { profile, runFile };
}

async function waitForVite(url, vite) {
  const deadline = Date.now() + READY_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (vite.exitCode !== null) throw new Error("Vite exited during startup");
    try {
      await fetch(url, { signal: AbortSignal.timeout(1_000) });
      return;
    } catch {
      await new Promise((wait) => setTimeout(wait, 200));
    }
  }
  throw new Error(`Vite did not answer on ${url}`);
}

const running = new Set();
let stopping = false;
let graceTimer;

/** Sweeps the trees once every child has exited, catching orphans too. */
function track(child) {
  running.add(child);
  child.once("exit", () => {
    running.delete(child);
    if (stopping && running.size === 0) {
      clearTimeout(graceTimer);
      killTrees();
    }
  });
  return child;
}

/**
 * Stops both children and everything they started. After Ctrl+C the console
 * has already signalled every process, so they get a grace period first.
 */
function stop(exitCode, { grace = false } = {}) {
  if (stopping) return;
  stopping = true;
  process.exitCode = exitCode;
  if (!grace) {
    killTrees();
    return;
  }
  signalRoots();
  graceTimer = setTimeout(killTrees, STOP_GRACE_MS);
}

function readRun(runFile) {
  let descriptor;
  try {
    descriptor = openSync(runFile, "r");
    const buffer = Buffer.alloc(MAX_RECORD_BYTES + 1);
    const bytes = readSync(descriptor, buffer, 0, buffer.length, 0);
    if (bytes > MAX_RECORD_BYTES) return undefined;
    return JSON.parse(buffer.toString("utf8", 0, bytes)).run;
  } catch {
    return undefined;
  } finally {
    if (descriptor !== undefined) closeSync(descriptor);
  }
}

/** Logs each new backend run, so a rebuild's finish is visible. */
function watchRuns(runFile) {
  let current;
  const timer = setInterval(() => {
    const run = readRun(runFile);
    if (run === undefined) return;
    // Pairing rewrites the record without its code; only a new run counts.
    if (run === current) return;
    current = run;
    console.log(
      `[sage] backend ready ${new Date().toLocaleTimeString()}; the tab pairs by itself`,
    );
  }, 500);
  timer.unref();
}

async function main() {
  refuseUnsupportedPlatform();
  try {
    // Inherited by both children and everything they start.
    os.setPriority(os.constants.priority.PRIORITY_BELOW_NORMAL);
  } catch {
    // Not permitted here; run at normal priority.
  }
  const port = await choosePort();
  const jobs = flag("jobs");
  if (jobs !== undefined && !/^[1-9]\d{0,2}$/.test(jobs))
    throw new Error("--jobs must be a positive integer");
  const devRoot = devStateDirectory();
  mkdirSync(devRoot, { recursive: true });
  if (existsSync(path.join(root, ".sage-dev")))
    console.log(
      "[sage] .sage-dev/ in this checkout is an old throwaway profile that is no longer used; delete it",
    );
  // Before anything touches the shared profile or its run record.
  const release = acquireLauncherLock(devRoot);
  process.on("exit", release);
  const { profile, runFile } = prepareProfile(devRoot);
  const origin = `http://127.0.0.1:${port}`;
  const config = path.join(devRoot, "tauri-dev.json");
  writeFileSync(
    config,
    JSON.stringify({ build: { devUrl: origin, beforeDevCommand: "" } }),
  );

  for (const signal of ["SIGINT", "SIGTERM", "SIGHUP", "SIGBREAK"])
    process.on(signal, () => stop(130, { grace: true }));

  const vite = track(
    launch(
      [
        path.join(root, "node_modules/vite/bin/vite.js"),
        "--host",
        "127.0.0.1",
        "--port",
        String(port),
        "--strictPort",
        "--logLevel",
        "warn",
      ],
      { cwd: root, env: { ...process.env, SAGE_DEV_BRIDGE_RUN_FILE: runFile } },
    ),
  );
  vite.once("exit", (code) => stop(code ?? 1));
  await waitForVite(origin, vite);

  // Like D:\SageWork\tools\dev-native.ps1: no inherited Sage or WebView2
  // settings may point the debug app at another profile.
  const tauriEnv = Object.fromEntries(
    Object.entries(process.env).filter(
      ([name]) => !/^(SAGE_|WEBVIEW2_)/i.test(name),
    ),
  );
  Object.assign(tauriEnv, {
    SAGE_DISPOSABLE_PROFILE_ROOT: profile,
    WEBVIEW2_USER_DATA_FOLDER: path.join(profile, "webview"),
    // Never the shared target a gate or another session may be holding.
    CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR || path.join(root, "target"),
    ...(jobs === undefined ? {} : { CARGO_BUILD_JOBS: jobs }),
  });
  const tauri = track(
    launch(
      [
        path.join(root, "node_modules/@tauri-apps/cli/tauri.js"),
        "dev",
        "--features",
        "browser-bridge",
        "--config",
        config,
      ],
      { cwd: root, env: tauriEnv },
    ),
  );
  tauri.once("exit", (code) => stop(code ?? 1));
  watchRuns(runFile);
  console.log(`\n  Sage (live backend): ${origin}/\n`);
  console.log(`[sage] throwaway profile: ${profile}`);
  console.log(
    `[sage] building the debug backend in ${tauriEnv.CARGO_TARGET_DIR}; the tab waits for it`,
  );
}

main().catch((error) => {
  console.error(`[sage] ${error instanceof Error ? error.message : error}`);
  stop(1);
});
