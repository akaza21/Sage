import fs from "node:fs";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";

const encoded = process.argv[2] ?? "";
if (encoded.length === 0 || encoded.length > 131_072) process.exit(64);

let request;
try {
  request = JSON.parse(Buffer.from(encoded, "base64url").toString("utf8"));
} catch {
  process.exit(65);
}

const {
  executablePath,
  arguments: args,
  timeoutMs,
  maxOutputBytes,
  resultPath,
} = request;
if (
  typeof executablePath !== "string" ||
  !path.isAbsolute(executablePath) ||
  !fs.statSync(executablePath).isFile() ||
  !Array.isArray(args) ||
  args.length > 32 ||
  args.some(
    (value) =>
      typeof value !== "string" || Buffer.byteLength(value, "utf8") > 4096,
  ) ||
  !Number.isInteger(timeoutMs) ||
  timeoutMs < 100 ||
  timeoutMs > 120_000 ||
  !Number.isInteger(maxOutputBytes) ||
  maxOutputBytes < 1 ||
  maxOutputBytes > 1_048_576 ||
  typeof resultPath !== "string" ||
  !path.isAbsolute(resultPath) ||
  fs.existsSync(resultPath)
) {
  process.exit(66);
}

const stdout = [];
const stderr = [];
let stdoutBytes = 0;
let stderrBytes = 0;
let terminalReason = null;
let spawnError = null;
let finished = false;

const child = spawn(executablePath, args, {
  windowsHide: true,
  stdio: ["ignore", "pipe", "pipe"],
});

function terminateTree() {
  if (process.platform === "win32") {
    const taskkill = path.join(
      process.env.SystemRoot ?? "C:\\Windows",
      "System32",
      "taskkill.exe",
    );
    const result = spawnSync(
      taskkill,
      ["/PID", String(child.pid), "/T", "/F"],
      {
        windowsHide: true,
        stdio: "ignore",
        timeout: 5000,
      },
    );
    if (!result.error && result.status === 0) return;
  }
  child.kill();
}

function capture(chunks, chunk, stream) {
  if (terminalReason) return;
  const next = (stream === "stdout" ? stdoutBytes : stderrBytes) + chunk.length;
  if (next > maxOutputBytes) {
    terminalReason = `${stream}-oversize`;
    terminateTree();
    return;
  }
  if (stream === "stdout") stdoutBytes = next;
  else stderrBytes = next;
  chunks.push(chunk);
}

child.stdout.on("data", (chunk) => capture(stdout, chunk, "stdout"));
child.stderr.on("data", (chunk) => capture(stderr, chunk, "stderr"));
child.on("error", (error) => {
  spawnError = error.message;
});

const timer = setTimeout(() => {
  terminalReason = terminalReason ?? "timeout";
  terminateTree();
}, timeoutMs);

child.on("close", (exitCode, signal) => {
  if (finished) return;
  finished = true;
  clearTimeout(timer);
  const result = {
    schemaVersion: 1,
    executablePath,
    exitCode,
    signal,
    terminalReason,
    spawnError,
    stdoutBytes,
    stderrBytes,
    stdout: Buffer.concat(stdout).toString("utf8"),
    stderr: Buffer.concat(stderr).toString("utf8"),
  };
  fs.writeFileSync(resultPath, `${JSON.stringify(result, null, 2)}\n`, {
    encoding: "utf8",
    flag: "wx",
  });
});
