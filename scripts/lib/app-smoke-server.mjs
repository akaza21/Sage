import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { connect } from "node:net";
import { resolve } from "node:path";

const READY_TIMEOUT_MS = 60_000;
const READY_POLL_MS = 500;
const EXIT_TIMEOUT_MS = 5_000;
const CLOSED_TIMEOUT_MS = 10_000;
const CLOSED_POLL_MS = 100;
const PROBE_TIMEOUT_MS = 1_000;
const REQUEST_TIMEOUT_MS = 2_000;
const OUTPUT_TAIL_LIMIT = 2_000;
const KILL_TIMEOUT_MS = 5_000;

function delay(ms) {
  return new Promise((done) => setTimeout(done, ms));
}

function within(promise, timeoutMs) {
  let timer;
  const expiry = new Promise((done) => {
    timer = setTimeout(() => done(false), timeoutMs);
  });
  return Promise.race([promise.then(() => true), expiry]).finally(() =>
    clearTimeout(timer),
  );
}

function captureOutput(child) {
  // truncated: the tail no longer starts at the output's first character.
  const output = { text: "", truncated: false };
  for (const stream of [child.stdout, child.stderr]) {
    if (!stream) continue;
    stream.setEncoding("utf8");
    stream.on("data", (chunk) => {
      const text = output.text + chunk;
      if (text.length > OUTPUT_TAIL_LIMIT) output.truncated = true;
      output.text = text.slice(-OUTPUT_TAIL_LIMIT);
    });
  }
  return output;
}

export function portAccepts(hostname, port) {
  return new Promise((done) => {
    const socket = connect({ host: hostname, port });
    const finish = (accepted) => {
      socket.removeAllListeners();
      socket.destroy();
      done(accepted);
    };
    socket.setTimeout(PROBE_TIMEOUT_MS, () => finish(false));
    socket.once("connect", () => finish(true));
    socket.once("error", () => finish(false));
  });
}

function killPosixTree(pid) {
  try {
    process.kill(-pid, "SIGKILL");
    return true;
  } catch {
    try {
      process.kill(pid, "SIGKILL");
      return true;
    } catch {
      return false;
    }
  }
}

export function killProcessTree(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return Promise.resolve(false);
  if (process.platform !== "win32") {
    return Promise.resolve(killPosixTree(pid));
  }
  return new Promise((done) => {
    const killer = spawn("taskkill", ["/PID", String(pid), "/T", "/F"], {
      stdio: "ignore",
      windowsHide: true,
    });
    const abandon = setTimeout(() => {
      killer.kill("SIGKILL");
      done(false);
    }, KILL_TIMEOUT_MS);
    const finish = (killed) => {
      clearTimeout(abandon);
      done(killed);
    };
    killer.once("error", () => finish(false));
    killer.once("close", (code) => finish(code === 0));
  });
}

export async function waitForServer(target, timeoutMs = READY_TIMEOUT_MS) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const response = await fetch(target, {
      signal: AbortSignal.timeout(
        Math.min(REQUEST_TIMEOUT_MS, deadline - Date.now()),
      ),
    }).catch(() => undefined);
    await response?.body?.cancel().catch(() => undefined);
    if (response?.ok) return;
    await delay(Math.min(READY_POLL_MS, deadline - Date.now()));
  }
  throw new Error(
    `dev server at ${target} did not start within ${timeoutMs}ms`,
  );
}

export async function waitForServerClosed(
  target,
  timeoutMs = CLOSED_TIMEOUT_MS,
) {
  const { hostname, port } = new URL(target);
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (!(await portAccepts(hostname, Number(port)))) return true;
    await delay(CLOSED_POLL_MS);
  }
  return !(await portAccepts(hostname, Number(port)));
}

export async function startDevServer(
  repositoryRoot,
  target,
  { readyTimeoutMs = READY_TIMEOUT_MS } = {},
) {
  const entry = resolve(
    repositoryRoot,
    "node_modules",
    "vite",
    "bin",
    "vite.js",
  );
  if (!existsSync(entry)) {
    throw new Error(`vite entrypoint is missing at ${entry}`);
  }
  const { hostname, port } = new URL(target);
  const child = spawn(
    process.execPath,
    [entry, "--host", hostname, "--port", port, "--strictPort"],
    {
      cwd: repositoryRoot,
      stdio: ["ignore", "pipe", "pipe"],
      shell: false,
      windowsHide: true,
      detached: process.platform !== "win32",
    },
  );
  const output = captureOutput(child);
  // The output tail stays readable, so a later failure can name what the
  // server reported.
  const server = { child, url: target, output };
  const pid = child.pid ?? null;
  try {
    await waitForServer(target, readyTimeoutMs);
  } catch (error) {
    const settled = await settleDevServer(server);
    const detail = output.text.trim();
    const parts = [error.message];
    if (!settled) parts.push(`dev server pid ${pid} did not settle`);
    if (detail) parts.push(detail);
    const failure = new Error(parts.join("\n"), { cause: error });
    failure.devServerPid = pid;
    failure.settled = settled;
    failure.server = server;
    throw failure;
  }
  return server;
}

export async function settleDevServer(server) {
  const child = server?.child;
  if (!child) return true;
  const pid = child.pid;
  const exited =
    child.exitCode !== null || child.signalCode !== null
      ? Promise.resolve()
      : new Promise((done) => child.once("exit", done));
  const reaped = await killProcessTree(pid);
  if (!reaped) child.kill("SIGKILL");
  let stopped = await within(exited, EXIT_TIMEOUT_MS);
  if (!stopped) {
    child.kill("SIGKILL");
    stopped = await within(exited, EXIT_TIMEOUT_MS);
  }
  const closed = await waitForServerClosed(server.url);
  const settled = stopped && closed;
  if (settled) server.child = undefined;
  return settled;
}
