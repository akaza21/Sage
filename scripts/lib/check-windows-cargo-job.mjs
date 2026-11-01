import { spawn } from "node:child_process";
import { randomBytes, randomUUID } from "node:crypto";
import { realpathSync, statSync } from "node:fs";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { CheckProcessError } from "./check-child-process.mjs";
import { consumeWindowsCargoJobFrames } from "./check-windows-cargo-protocol.mjs";

const MAX_REQUEST_BYTES = 256 * 1_024;
const MAX_ENVIRONMENT_ENTRIES = 512;
const MAX_ENVIRONMENT_BYTES = 128 * 1_024;
const MAX_ARGUMENTS = 128;
const MAX_ARGUMENT_BYTES = 128 * 1_024;
const MAX_ITEM_BYTES = 16 * 1_024;
const MAX_COMMAND_TIMEOUT_MS = 3 * 60 * 60_000;
const MAX_TERMINATION_GRACE_MS = 5_000;
const MAX_SETTLEMENT_TIMEOUT_MS = 60_000;
const MAX_LINE_BYTES = 1024 * 1_024;
const MAX_OUTPUT_BYTES = 16 * 1024 * 1024;
const helper = fileURLToPath(new URL("windows-cargo-job.ps1", import.meta.url));
const modulePath = fileURLToPath(
  new URL("WindowsCargoJob.psm1", import.meta.url),
);
const sourceRoot = realpathSync(
  path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", ".."),
);

function repositoryFile(candidate) {
  const physical = realpathSync(candidate);
  const relative = path.relative(sourceRoot, physical);
  if (
    relative.startsWith("..") ||
    path.isAbsolute(relative) ||
    !statSync(physical).isFile()
  ) {
    throw new Error("Job helper escaped its source checkout");
  }
  return physical;
}

function powershellPath(environment) {
  const systemRoot = environment.SystemRoot ?? environment.SYSTEMROOT;
  if (!systemRoot || !path.isAbsolute(systemRoot)) {
    throw new Error("SystemRoot is unavailable");
  }
  return realpathSync(
    path.join(
      systemRoot,
      "System32",
      "WindowsPowerShell",
      "v1.0",
      "powershell.exe",
    ),
  );
}

function resolveExecutable(command, environment) {
  if (path.isAbsolute(command)) return realpathSync(command);
  if (path.basename(command) !== command) {
    throw new Error("relative executable must be a PATH leaf");
  }
  const extensions = (environment.PATHEXT ?? ".EXE;.CMD;.BAT;.COM").split(";");
  for (const directory of (environment.PATH ?? "").split(path.delimiter)) {
    if (!directory) continue;
    const candidates = path.extname(command)
      ? [command]
      : extensions.map((extension) => `${command}${extension}`);
    for (const candidateName of candidates) {
      try {
        const physicalDirectory = realpathSync(directory);
        const candidate = path.join(physicalDirectory, candidateName);
        const physicalCandidate = realpathSync(candidate);
        if (statSync(physicalCandidate).isFile()) return candidate;
      } catch {
        // Continue through the finite PATH/PATHEXT candidate set.
      }
    }
  }
  throw new Error("executable unavailable");
}

function validateItem(value, kind) {
  if (
    typeof value !== "string" ||
    value.includes("\0") ||
    Buffer.byteLength(value, "utf8") > MAX_ITEM_BYTES
  ) {
    throw new Error(`${kind} is invalid`);
  }
  return value;
}

function mergedEnvironment(environment, overrides = {}) {
  const entries = new Map();
  for (const [name, value] of [
    ...Object.entries(environment),
    ...Object.entries(overrides),
  ]) {
    if (value === undefined) continue;
    if (!name || name.includes("=") || name.includes("\0")) {
      throw new Error("environment name is invalid");
    }
    entries.set(name.toLowerCase(), [name, validateItem(value, "environment")]);
  }
  if (!entries.has("cargo_build_jobs")) {
    entries.set("cargo_build_jobs", ["CARGO_BUILD_JOBS", "32"]);
  }
  const values = [...entries.values()]
    .sort(([left], [right]) =>
      left.localeCompare(right, "en", { sensitivity: "base" }),
    )
    .map(([name, value]) => `${name}=${value}`);
  const bytes = values.reduce(
    (total, value) => total + Buffer.byteLength(value, "utf8"),
    0,
  );
  if (
    values.length > MAX_ENVIRONMENT_ENTRIES ||
    bytes > MAX_ENVIRONMENT_BYTES
  ) {
    throw new Error("environment aggregate is invalid");
  }
  return values;
}

function buildRequest(command, environment, controlPipe, controlNonce) {
  for (const [value, maximum, name] of [
    [command.timeoutMs, MAX_COMMAND_TIMEOUT_MS, "timeout"],
    [command.terminationGraceMs, MAX_TERMINATION_GRACE_MS, "termination grace"],
    [
      command.settlementTimeoutMs,
      MAX_SETTLEMENT_TIMEOUT_MS,
      "settlement timeout",
    ],
    [command.maximumLineBytes, MAX_LINE_BYTES, "line bound"],
    [command.maximumOutputBytes, MAX_OUTPUT_BYTES, "output bound"],
  ]) {
    if (!Number.isSafeInteger(value) || value < 1 || value > maximum) {
      throw new Error(`${name} is invalid`);
    }
  }
  validateItem(command.label, "label");
  const arguments_ = command.args.map((value) =>
    validateItem(value, "argument"),
  );
  const argumentBytes = arguments_.reduce(
    (total, value) => total + Buffer.byteLength(value, "utf8"),
    0,
  );
  if (arguments_.length > MAX_ARGUMENTS || argumentBytes > MAX_ARGUMENT_BYTES) {
    throw new Error("argument aggregate is invalid");
  }
  const requestNonce = randomUUID();
  const request = JSON.stringify({
    schemaVersion: 1,
    requestNonce,
    controlNonce,
    controlPipe,
    executablePath: resolveExecutable(command.command, environment),
    arguments: arguments_,
    cwd: realpathSync(command.cwd),
    environment: mergedEnvironment(environment, command.env),
    deadlineEpochMs: Date.now() + command.timeoutMs,
    terminationGraceMs: command.terminationGraceMs,
    settlementTimeoutMs: command.settlementTimeoutMs,
    maximumLineBytes: command.maximumLineBytes,
    maximumOutputBytes: command.maximumOutputBytes,
  });
  if (Buffer.byteLength(request, "utf8") > MAX_REQUEST_BYTES) {
    throw new Error("request is oversized");
  }
  return { request, requestNonce };
}

function responseError(command, response, signal) {
  let code = "command-failed";
  if (response.teardown?.kind === "msvc-vctip-termination-unconfirmed") {
    code = "windows-job-vctip-termination-unconfirmed";
  }
  if (response.reason === "command-timeout") code = "command-timeout";
  if (response.reason === "command-aborted") code = "command-aborted";
  if (response.reason === "output-overflow") code = "output-overflow";
  if (!response.settled || response.reason === "tree-repopulated") {
    code = "windows-job-settlement-unconfirmed";
  }
  const error = new CheckProcessError(
    `${command.label} failed in its Windows Job`,
    {
      code,
      settled: response.settled,
      cause: code === "command-aborted" ? signal.reason : undefined,
    },
  );
  error.exitCode = response.exitCode;
  error.teardown = response.teardown;
  error.stall = response.stall;
  return error;
}

export function executeWindowsCargoJobCommand(
  command,
  signal,
  {
    environment = process.env,
    stdout = process.stdout,
    stderr = process.stderr,
  } = {},
) {
  return new Promise((resolve, reject) => {
    if (signal.aborted) {
      reject(
        new CheckProcessError(`${command.label} was aborted before launch`, {
          code: "command-aborted",
          settled: true,
          cause: signal.reason,
        }),
      );
      return;
    }
    const controlPipe = `\\\\.\\pipe\\sage-cargo-${randomUUID()}`;
    const controlNonce = randomBytes(32).toString("hex");
    let requestData;
    try {
      requestData = buildRequest(
        command,
        environment,
        controlPipe,
        controlNonce,
      );
    } catch (cause) {
      reject(
        new CheckProcessError("Windows Cargo Job request is invalid", {
          code: "windows-job-request-invalid",
          cause,
          settled: true,
        }),
      );
      return;
    }

    let child;
    let controlSocket;
    let diagnosticBytes = 0;
    let finished = false;
    let pendingAbort = false;
    let wrapperTimer;
    const controlServer = net.createServer((socket) => {
      if (controlSocket) return socket.destroy();
      controlSocket = socket;
      socket.on("error", () => {});
      if (pendingAbort) socket.end(`ABORT ${controlNonce}\n`);
    });
    const closeControl = () => {
      controlSocket?.destroy();
      controlServer.close();
    };
    const finish = (error, result) => {
      if (finished) return;
      finished = true;
      clearTimeout(wrapperTimer);
      signal.removeEventListener("abort", abort);
      closeControl();
      if (error) reject(error);
      else resolve(result);
    };
    const unavailable = (cause) =>
      finish(
        new CheckProcessError("Windows Cargo Job proof was unavailable", {
          code: "windows-job-proof-unavailable",
          cause,
          settled: false,
        }),
      );
    const abort = () => {
      pendingAbort = true;
      controlSocket?.end(`ABORT ${controlNonce}\n`);
    };
    signal.addEventListener("abort", abort, { once: true });
    controlServer.once("error", (cause) =>
      finish(
        new CheckProcessError(
          "Windows Cargo control pipe failed before launch",
          {
            code: "windows-job-control-failed",
            cause,
            settled: child ? false : true,
          },
        ),
      ),
    );
    controlServer.listen(controlPipe, () => {
      try {
        child = spawn(
          powershellPath(environment),
          [
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            repositoryFile(helper),
            "-ModulePath",
            repositoryFile(modulePath),
          ],
          { shell: false, windowsHide: true, stdio: ["pipe", "pipe", "pipe"] },
        );
      } catch (cause) {
        unavailable(cause);
        return;
      }
      const frames = consumeWindowsCargoJobFrames(
        child.stdout,
        command,
        requestData.requestNonce,
        stdout,
        stderr,
      );
      frames.catch(() => child?.kill());
      child.stderr.on("data", (chunk) => {
        diagnosticBytes += chunk.length;
        if (diagnosticBytes > 64 * 1_024) child.kill();
      });
      child.once("error", unavailable);
      child.once("close", async (code) => {
        if (finished) return;
        if (code !== 0) {
          return unavailable(new Error("wrapper-failed"));
        }
        try {
          const result = await frames;
          if (
            !result.settled ||
            result.reason ||
            result.exitCode !== 0 ||
            result.teardown?.kind === "msvc-vctip-termination-unconfirmed"
          ) {
            finish(responseError(command, result, signal));
          } else finish(null, result);
        } catch (cause) {
          unavailable(cause);
        }
      });
      child.stdin.end(`${requestData.request}\n`);
      wrapperTimer = setTimeout(
        () => {
          child.kill();
          unavailable(new Error("wrapper proof deadline exceeded"));
        },
        command.timeoutMs +
          command.terminationGraceMs +
          command.settlementTimeoutMs +
          2_000,
      );
    });
  });
}
