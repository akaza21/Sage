import { spawn } from "node:child_process";

import { posixGroupAlive } from "./process-liveness.mjs";

const TASKKILL_TIMEOUT_MS = 2_000;
const SETTLEMENT_POLL_MS = 25;

export class CheckProcessError extends Error {
  constructor(message, { code, cause, settled } = {}) {
    super(message, { cause });
    this.name = "CheckProcessError";
    this.code = code;
    if (settled !== undefined) this.settled = settled;
  }
}

function assertCommandLimits(command) {
  for (const name of [
    "timeoutMs",
    "terminationGraceMs",
    "settlementTimeoutMs",
    "maximumLineBytes",
  ]) {
    if (!Number.isSafeInteger(command[name]) || command[name] < 1) {
      throw new CheckProcessError(`${command.label} has invalid ${name}`, {
        code: "command-limits-invalid",
      });
    }
  }
  if (
    command.maximumOutputBytes !== undefined &&
    (!Number.isSafeInteger(command.maximumOutputBytes) ||
      command.maximumOutputBytes < 1)
  ) {
    throw new CheckProcessError(
      `${command.label} has invalid maximumOutputBytes`,
      { code: "command-limits-invalid" },
    );
  }
}

function runBoundedTaskkill(pid, force, availableMs = TASKKILL_TIMEOUT_MS) {
  if (availableMs < 1) {
    return Promise.resolve({ success: false, timedOut: true });
  }
  return new Promise((resolve) => {
    const arguments_ = ["/PID", String(pid), "/T"];
    if (force) arguments_.push("/F");
    const killer = spawn("taskkill.exe", arguments_, {
      shell: false,
      stdio: "ignore",
      windowsHide: true,
    });
    let completed = false;
    const finish = (result) => {
      if (completed) return;
      completed = true;
      clearTimeout(timer);
      resolve(result);
    };
    const timer = setTimeout(
      () => {
        killer.kill("SIGKILL");
        finish({ success: false, timedOut: true });
      },
      Math.min(TASKKILL_TIMEOUT_MS, availableMs),
    );
    timer.unref();
    killer.once("error", () => finish({ success: false, timedOut: false }));
    killer.once("close", (code) =>
      finish({ success: code === 0, timedOut: false }),
    );
  });
}

function signalPosixGroup(processGroupId, signal) {
  try {
    process.kill(-processGroupId, signal);
    return true;
  } catch (error) {
    if (error.code === "ESRCH") return false;
    throw error;
  }
}

async function waitUntil(predicate, deadline) {
  while (Date.now() < deadline) {
    if (predicate()) return true;
    await new Promise((resolve) => setTimeout(resolve, SETTLEMENT_POLL_MS));
  }
  return predicate();
}

async function delayUntil(deadline) {
  const remaining = deadline - Date.now();
  if (remaining > 0) {
    await new Promise((resolve) => setTimeout(resolve, remaining));
  }
}

function attachBoundedOutput({
  source,
  stream,
  label,
  maximumLineBytes,
  maximumOutputBytes,
  onOverflow,
}) {
  let remainder = Buffer.alloc(0);
  let totalBytes = 0;
  let overflowed = false;
  const write = (bytes, suffix = "") => {
    if (!stream.write(`[${label}] ${bytes.toString("utf8")}${suffix}\n`)) {
      source.pause();
      stream.once("drain", () => source.resume());
    }
  };
  const overflow = (kind) => {
    if (overflowed) return;
    overflowed = true;
    onOverflow(kind);
  };
  const onData = (chunk) => {
    if (overflowed) return;
    totalBytes += chunk.byteLength;
    if (maximumOutputBytes && totalBytes > maximumOutputBytes) {
      overflow("total");
      return;
    }
    let offset = 0;
    while (offset < chunk.byteLength) {
      const newline = chunk.indexOf(10, offset);
      const end = newline < 0 ? chunk.byteLength : newline;
      let segment = chunk.subarray(offset, end);
      if (remainder.byteLength + segment.byteLength > maximumLineBytes) {
        segment = segment.subarray(
          0,
          Math.max(0, maximumLineBytes - remainder.byteLength),
        );
        write(
          Buffer.concat([remainder, segment], maximumLineBytes),
          "…[truncated]",
        );
        remainder = Buffer.alloc(0);
        overflow("line");
        return;
      }
      if (newline < 0) {
        remainder = Buffer.concat([remainder, segment]);
        return;
      }
      let line = Buffer.concat([remainder, segment]);
      if (line.at(-1) === 13) line = line.subarray(0, line.byteLength - 1);
      write(line);
      remainder = Buffer.alloc(0);
      offset = newline + 1;
    }
  };
  source.on("data", onData);
  return {
    flush() {
      if (!overflowed && remainder.byteLength > 0) write(remainder);
      remainder = Buffer.alloc(0);
    },
    detach() {
      source.removeListener("data", onData);
      source.destroy();
      remainder = Buffer.alloc(0);
    },
  };
}

async function settleWindowsTree(pid, leader, startedAt, command) {
  const deadline = startedAt + command.settlementTimeoutMs;
  const graceDeadline = Math.min(
    deadline,
    startedAt + command.terminationGraceMs,
  );
  const graceful = await runBoundedTaskkill(
    pid,
    false,
    graceDeadline - Date.now(),
  );
  let treeProven = graceful.success;
  if (!treeProven) {
    await delayUntil(graceDeadline);
    const forced = await runBoundedTaskkill(pid, true, deadline - Date.now());
    treeProven = forced.success;
  }
  const leaderProven = await waitUntil(() => leader.closed, deadline);
  return treeProven && leaderProven;
}

async function settlePosixTree(processGroupId, leader, startedAt, command) {
  const deadline = startedAt + command.settlementTimeoutMs;
  const graceDeadline = Math.min(
    deadline,
    startedAt + command.terminationGraceMs,
  );
  signalPosixGroup(processGroupId, "SIGTERM");
  const goneDuringGrace = await waitUntil(
    () => !posixGroupAlive(processGroupId),
    graceDeadline,
  );
  if (!goneDuringGrace) signalPosixGroup(processGroupId, "SIGKILL");
  const groupProven =
    goneDuringGrace ||
    (await waitUntil(() => !posixGroupAlive(processGroupId), deadline));
  const leaderProven = await waitUntil(() => leader.closed, deadline);
  return groupProven && leaderProven;
}

export function executeCommand(
  command,
  signal,
  {
    environment = process.env,
    platform = process.platform,
    stdout = process.stdout,
    stderr = process.stderr,
  } = {},
) {
  assertCommandLimits(command);
  return new Promise((resolve, reject) => {
    if (signal.aborted) {
      reject(
        new CheckProcessError(`${command.label} was aborted before launch`, {
          code: "command-aborted",
          cause: signal.reason,
          settled: true,
        }),
      );
      return;
    }
    const child = spawn(command.command, command.args, {
      cwd: command.cwd,
      detached: platform !== "win32",
      env: { ...environment, ...command.env },
      shell: false,
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
    });
    const leader = { closed: false, code: undefined, signal: undefined };
    let finished = false;
    let terminalError;
    let commandTimer;
    const finish = (error) => {
      if (finished) return;
      finished = true;
      clearTimeout(commandTimer);
      signal.removeEventListener("abort", abort);
      if (error) reject(error);
      else resolve();
    };
    const settle = async (error) => {
      const startedAt = Date.now();
      let settled = false;
      try {
        settled =
          platform === "win32"
            ? await settleWindowsTree(child.pid, leader, startedAt, command)
            : await settlePosixTree(child.pid, leader, startedAt, command);
      } catch (settlementError) {
        error.cause ??= settlementError;
      }
      if (!settled) {
        stdoutState.detach();
        stderrState.detach();
      }
      error.settled = settled;
      finish(error);
    };
    const stop = (error) => {
      if (terminalError || finished) return;
      terminalError = error;
      clearTimeout(commandTimer);
      void settle(error);
    };
    const overflow = (streamName, kind) =>
      stop(
        new CheckProcessError(
          `${command.label} ${streamName} exceeded its bounded ${kind} output`,
          { code: "output-overflow" },
        ),
      );
    const stdoutState = attachBoundedOutput({
      source: child.stdout,
      stream: stdout,
      label: command.label,
      maximumLineBytes: command.maximumLineBytes,
      maximumOutputBytes: command.maximumOutputBytes,
      onOverflow: (kind) => overflow("stdout", kind),
    });
    const stderrState = attachBoundedOutput({
      source: child.stderr,
      stream: stderr,
      label: command.label,
      maximumLineBytes: command.maximumLineBytes,
      maximumOutputBytes: command.maximumOutputBytes,
      onOverflow: (kind) => overflow("stderr", kind),
    });
    const abort = () =>
      stop(
        new CheckProcessError(`${command.label} was aborted`, {
          code: "command-aborted",
          cause: signal.reason,
        }),
      );
    signal.addEventListener("abort", abort, { once: true });
    commandTimer = setTimeout(
      () =>
        stop(
          new CheckProcessError(`${command.label} exceeded its deadline`, {
            code: "command-timeout",
          }),
        ),
      command.timeoutMs,
    );
    child.once("error", (error) => {
      stdoutState.detach();
      stderrState.detach();
      finish(
        new CheckProcessError(
          `${command.label} failed to start: ${error.message}`,
          {
            code: "command-start-failed",
            cause: error,
            settled: true,
          },
        ),
      );
    });
    child.once("close", (code, childSignal) => {
      leader.closed = true;
      leader.code = code;
      leader.signal = childSignal;
      stdoutState.flush();
      stderrState.flush();
      if (terminalError) return;
      if (code === 0) finish();
      else {
        stop(
          new CheckProcessError(
            `${command.label} failed (${childSignal ? `signal ${childSignal}` : `exit ${code}`})`,
            { code: "command-failed" },
          ),
        );
      }
    });
  });
}
