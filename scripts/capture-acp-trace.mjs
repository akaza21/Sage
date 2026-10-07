import { spawn } from "node:child_process";
import { mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve, sep } from "node:path";

const [command, argsRaw, outPath, cwd, promptText] = process.argv.slice(2);
if (!command || !outPath) {
  console.error(
    "usage: capture-acp-trace.mjs <command> <comma-args> <out.jsonl> [cwd] [prompt]",
  );
  process.exit(2);
}

const args = argsRaw ? argsRaw.split(",").filter(Boolean) : [];
const workdir = resolve(cwd || process.cwd());
const prompt = promptText || "Reply with the single word ok.";
const frames = [];

const CURSOR_VERSION_DIR =
  /^\d{4}\.\d{1,2}\.\d{1,2}(-\d{2}-\d{2}-\d{2})?-[a-f0-9]+$/;

const REDACTED = "[redacted]";
const EMAIL = /[\w.+-]+@[\w-]+\.[\w.-]+/g;

const redactAuthStatus = (frame) => {
  const status = frame.params && frame.params.authStatus;
  if (!status || typeof status !== "object") return frame;
  const redactedStatus = { ...status };
  if (typeof status.label === "string") redactedStatus.label = REDACTED;
  if (status.account && typeof status.account === "object") {
    redactedStatus.account = Object.fromEntries(
      Object.keys(status.account).map((key) => [key, REDACTED]),
    );
  }
  return {
    ...frame,
    params: { ...frame.params, authStatus: redactedStatus },
  };
};

const redactEntry = (entry) =>
  entry.frame && entry.frame.method === "_auth/status_update"
    ? { ...entry, frame: redactAuthStatus(entry.frame) }
    : entry;

const cursorVersions = () => {
  if (!/cursor-agent/i.test(command)) return undefined;
  const root = resolve(
    process.env.LOCALAPPDATA || process.cwd(),
    "cursor-agent",
    "versions",
  );
  let entries;
  try {
    entries = readdirSync(root, { withFileTypes: true });
  } catch {
    return { root, error: "unreadable" };
  }
  const ranked = entries
    .filter(
      (entry) => entry.isDirectory() && CURSOR_VERSION_DIR.test(entry.name),
    )
    .map((entry) => {
      const [year, month, day] = entry.name.split("-")[0].split(".");
      return {
        name: entry.name,
        rank: Number(year + month.padStart(2, "0") + day.padStart(2, "0")),
      };
    })
    .sort((a, b) => b.rank - a.rank);
  const tied = ranked.filter((entry) => entry.rank === ranked[0]?.rank);
  return {
    root,
    selected: ranked[0]?.name,
    ambiguous: tied.length > 1,
    candidates: ranked.map((entry) => entry.name),
  };
};

const child = spawn(command, args, {
  cwd: workdir,
  stdio: ["pipe", "pipe", "pipe"],
  shell: process.platform === "win32",
});

let buffer = "";
let stderr = "";
child.stderr.on("data", (chunk) => {
  stderr += chunk.toString();
});

const send = (value) => {
  frames.push({ direction: "out", frame: value });
  child.stdin.write(JSON.stringify(value) + "\n");
};

let sessionId = null;
let done = false;

const finish = (reason) => {
  if (done) return;
  done = true;
  mkdirSync(dirname(resolve(outPath)), { recursive: true });
  const lines = frames
    .map((entry) => JSON.stringify(redactEntry(entry)))
    .join("\n");
  writeFileSync(resolve(outPath), lines + "\n");
  writeFileSync(
    resolve(outPath) + ".meta.json",
    JSON.stringify(
      {
        command,
        args,
        reason,
        cursorVersions: cursorVersions(),
        stderr: stderr.slice(-4000).replace(EMAIL, REDACTED),
      },
      null,
      2,
    ),
  );
  child.kill();
  console.log(reason + " frames=" + frames.length);
  process.exit(0);
};

child.stdout.on("data", (chunk) => {
  buffer += chunk.toString();
  let index = buffer.indexOf("\n");
  while (index >= 0) {
    const line = buffer.slice(0, index).trim();
    buffer = buffer.slice(index + 1);
    index = buffer.indexOf("\n");
    if (!line) continue;
    let value;
    try {
      value = JSON.parse(line);
    } catch {
      frames.push({ direction: "in", raw: line });
      continue;
    }
    frames.push({ direction: "in", frame: value });
    handle(value);
  }
});

function handle(value) {
  if (typeof value.id !== "undefined" && value.method) {
    respond(value);
    return;
  }
  if (value.id === 1 && value.result) {
    send({ jsonrpc: "2.0", method: "initialized", params: {} });
    send({
      jsonrpc: "2.0",
      id: 2,
      method: "session/new",
      params: { cwd: workdir, mcpServers: [] },
    });
    return;
  }
  if (value.id === 1 && value.error) {
    finish("initialize-error");
    return;
  }
  if (value.id === 2) {
    if (value.error) {
      finish("session-new-error");
      return;
    }
    sessionId = value.result?.sessionId;
    send({
      jsonrpc: "2.0",
      id: 3,
      method: "session/prompt",
      params: { sessionId, prompt: [{ type: "text", text: prompt }] },
    });
    return;
  }
  if (value.id === 3) {
    finish(value.error ? "prompt-error" : "prompt-complete");
    return;
  }
}

function respond(value) {
  const params = value.params || {};
  if (value.method === "session/request_permission") {
    const options = params.options || [];
    const chosen =
      options.find((option) => option.kind === "allow_always") ||
      options.find((option) => option.kind === "allow_once") ||
      options[0];
    if (chosen) {
      send({
        jsonrpc: "2.0",
        id: value.id,
        result: { outcome: { outcome: "selected", optionId: chosen.optionId } },
      });
      return;
    }
  }
  if (value.method === "fs/read_text_file") {
    try {
      const full = resolve(workdir, params.path);
      if (full !== workdir && !full.startsWith(workdir + sep)) {
        throw new Error("path escapes capture working directory");
      }
      send({
        jsonrpc: "2.0",
        id: value.id,
        result: { content: readFileSync(full, "utf8") },
      });
      return;
    } catch (error) {
      send({
        jsonrpc: "2.0",
        id: value.id,
        error: { code: -32000, message: String(error && error.message) },
      });
      return;
    }
  }
  send({
    jsonrpc: "2.0",
    id: value.id,
    error: {
      code: -32601,
      message: "capture client does not implement " + value.method,
    },
  });
}

child.on("error", () => finish("spawn-error"));
child.on("exit", () => finish("child-exit"));

send({
  jsonrpc: "2.0",
  id: 1,
  method: "initialize",
  params: {
    protocolVersion: 1,
    clientCapabilities: { fs: { readTextFile: true, writeTextFile: false } },
    clientInfo: { name: "sage-trace-capture", version: "1" },
  },
});

setTimeout(() => finish("timeout"), 90000);
