#!/usr/bin/env node
// `npm run dev`: the real UI against the real debug backend.
//
//   npm run dev [-- --port <n>] [-- --jobs <n>]
//
// Starts Vite on a free loopback port (or --port / SAGE_UI_PORT), then
// `tauri dev --features browser-bridge` with a generated config that points
// at that Vite and runs no beforeDevCommand. The backend uses a throwaway
// profile under .sage-dev/ in this checkout, never the real Sage profile, and
// publishes each run there; the dev server hands it to the tab, which pairs by
// itself and reloads after a Rust rebuild. Prints one URL. Both children run
// at below-normal priority and stop together.
import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const STOP_GRACE_MS = 5_000;
const READY_TIMEOUT_MS = 60_000;
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

function prepareProfile() {
  const devRoot = path.join(root, ".sage-dev");
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
  return { devRoot, profile, runFile };
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

const children = new Set();

function launch(args, env) {
  const child = spawn(process.execPath, args, {
    cwd: root,
    env,
    stdio: ["ignore", "inherit", "inherit"],
    // POSIX: own process group, so stopping reaches cargo and the app too.
    detached: process.platform !== "win32",
  });
  children.add(child);
  child.once("exit", () => children.delete(child));
  return child;
}

function signalTree(child, force) {
  if (child.exitCode !== null || child.pid === undefined) return;
  if (process.platform === "win32") {
    // Without /F the app window gets a close request and can shut down cleanly.
    spawnSync(
      "taskkill",
      ["/pid", String(child.pid), "/T", ...(force ? ["/F"] : [])],
      {
        stdio: "ignore",
      },
    );
  } else {
    try {
      process.kill(-child.pid, force ? "SIGKILL" : "SIGTERM");
    } catch {
      // Already gone.
    }
  }
}

let stopping = false;
function stop(exitCode) {
  if (stopping) return;
  stopping = true;
  process.exitCode = exitCode;
  for (const child of children) signalTree(child, false);
  setTimeout(() => {
    for (const child of children) signalTree(child, true);
  }, STOP_GRACE_MS).unref();
}

/** Logs each new backend run, so a rebuild's finish is visible. */
function watchRuns(runFile) {
  let current;
  const timer = setInterval(() => {
    let run;
    try {
      run = JSON.parse(readFileSync(runFile, "utf8")).run;
    } catch {
      return;
    }
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
  const { devRoot, profile, runFile } = prepareProfile();
  const origin = `http://127.0.0.1:${port}`;
  const config = path.join(devRoot, "tauri-dev.json");
  writeFileSync(
    config,
    JSON.stringify({ build: { devUrl: origin, beforeDevCommand: "" } }),
  );

  for (const signal of ["SIGINT", "SIGTERM", "SIGHUP", "SIGBREAK"])
    process.on(signal, () => stop(130));

  const vite = launch(
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
    { ...process.env, SAGE_DEV_BRIDGE_RUN_FILE: runFile },
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
    ...(jobs === undefined ? {} : { CARGO_BUILD_JOBS: jobs }),
  });
  const tauri = launch(
    [
      path.join(root, "node_modules/@tauri-apps/cli/tauri.js"),
      "dev",
      "--features",
      "browser-bridge",
      "--config",
      config,
    ],
    tauriEnv,
  );
  tauri.once("exit", (code) => stop(code ?? 1));
  watchRuns(runFile);
  console.log(`\n  Sage (live backend): ${origin}/\n`);
  console.log("[sage] building the debug backend; the tab waits for it");
}

main().catch((error) => {
  console.error(`[sage] ${error instanceof Error ? error.message : error}`);
  stop(1);
});
