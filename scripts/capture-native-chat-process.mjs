import { spawn, execFile } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import {
  closeSync,
  existsSync,
  lstatSync,
  mkdirSync,
  openSync,
  readSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import net from "node:net";
import path from "node:path";
import { NATIVE_IMAGE_BASE64 } from "./acp-native-case-events.mjs";

export function runTool(executable, args, timeout = 10000) {
  return new Promise((resolve, reject) =>
    execFile(
      executable,
      args,
      { encoding: "utf8", timeout, maxBuffer: 65536, windowsHide: true },
      (error, stdout) =>
        error
          ? reject(new Error("native-tool-blocked"))
          : resolve(stdout.trim()),
    ),
  );
}
export function requirePlain(file, directory = false) {
  const absolute = path.resolve(file);
  let current = path.parse(absolute).root;
  for (const part of absolute
    .slice(current.length)
    .split(path.sep)
    .filter(Boolean)) {
    current = path.join(current, part);
    const metadata = lstatSync(current);
    if (metadata.isSymbolicLink()) throw new Error("path-reparse");
  }
  const metadata = lstatSync(absolute);
  if (directory ? !metadata.isDirectory() : !metadata.isFile())
    throw new Error("path-kind");
  return realpathSync(absolute);
}
export function ensurePlainDirectory(directory) {
  const absolute = path.resolve(directory);
  let current = path.parse(absolute).root;
  requirePlain(current, true);
  for (const part of absolute
    .slice(current.length)
    .split(path.sep)
    .filter(Boolean)) {
    current = path.join(current, part);
    if (!existsSync(current)) mkdirSync(current);
    requirePlain(current, true);
  }
  return absolute;
}

export function executableIdentity(file) {
  const resolved = requirePlain(file),
    handle = openSync(resolved, "r"),
    digest = createHash("sha256"),
    chunk = Buffer.alloc(65536);
  let bytes = 0;
  try {
    for (;;) {
      const count = readSync(handle, chunk, 0, chunk.length, null);
      if (count === 0) break;
      bytes += count;
      if (bytes > 512 * 1024 * 1024) throw new Error("artifact-bound");
      digest.update(chunk.subarray(0, count));
    }
  } finally {
    closeSync(handle);
  }
  if (bytes === 0) throw new Error("artifact-empty");
  return { path: resolved, bytes, sha256: digest.digest("hex") };
}
export async function reservePort(requestedPort = 0) {
  const server = net.createServer();
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(requestedPort, "127.0.0.1", resolve);
  });
  const port = server.address().port;
  await new Promise((resolve, reject) =>
    server.close((error) => (error ? reject(error) : resolve())),
  );
  return port;
}
export function freshRoots(workspace) {
  requirePlain(workspace, true);
  const proof = path.join(workspace, "work", "cp-native-proof");
  ensurePlainDirectory(proof);
  requirePlain(proof, true);
  const owned = path.join(proof, "nc1-" + randomUUID());
  mkdirSync(owned);
  const profile = path.join(owned, "profile"),
    project = path.join(owned, "project");
  mkdirSync(profile);
  mkdirSync(path.join(project, "src"), { recursive: true });
  writeFileSync(
    path.join(project, "src", "example.png"),
    Buffer.from(NATIVE_IMAGE_BASE64, "base64"),
    { flag: "wx" },
  );
  writeFileSync(
    path.join(profile, ".sage-disposable-profile-v1"),
    "sage-disposable-profile-v1\n",
    { flag: "wx" },
  );
  for (const name of ["example", "example-1", "example-2", "example-3"])
    writeFileSync(
      path.join(project, "src", `${name}.ts`),
      "export const example = 1;\n",
      { flag: "wx" },
    );
  return { owned, profile, project, proof };
}
export function launchOwned(build, fixture, roots, options, port) {
  const environment = { ...process.env };
  for (const key of Object.keys(environment))
    if (
      key.startsWith("SAGE_") ||
      key === "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS" ||
      key === "WEBVIEW2_USER_DATA_FOLDER"
    )
      delete environment[key];
  const prefix = {
    codex: "SAGE_CODEX_FIXTURE",
    claude: "SAGE_CLAUDE_FIXTURE",
    pi: "SAGE_PI_FIXTURE",
  }[options.harness];
  Object.assign(environment, {
    SAGE_DISPOSABLE_PROFILE_ROOT: roots.profile,
    SAGE_PROJECT_ROOT: roots.project,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:
      "--remote-debugging-address=127.0.0.1 --remote-debugging-port=" + port,
    ...(fixture
      ? {
          [prefix + "_PATH"]: fixture.path,
          [prefix + "_SHA256"]: fixture.sha256,
          [prefix + "_BYTES"]: String(fixture.bytes),
          [prefix + "_CASE"]: options.caseId,
        }
      : {}),
  });
  const child = spawn(build.path, [], {
    cwd: roots.project,
    env: environment,
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  child.on("error", () => {});
  return child;
}
export async function stopOwned(child) {
  if (
    child &&
    child.exitCode === null &&
    child.signalCode === null &&
    child.pid
  ) {
    await runTool("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"]);
    await new Promise((resolve, reject) => {
      if (child.exitCode !== null || child.signalCode !== null)
        return resolve();
      const timeout = setTimeout(
        () => reject(new Error("owned-cleanup-timeout")),
        10000,
      );
      child.once("exit", () => {
        clearTimeout(timeout);
        resolve();
      });
    });
  }
}
export async function cleanupRoots(
  roots,
  {
    remove = rmSync,
    pause = (delay) => new Promise((resolve) => setTimeout(resolve, delay)),
  } = {},
) {
  for (let attempt = 1; attempt <= 21; attempt++) {
    if (!roots || !existsSync(roots.owned)) return;
    try {
      const resolved = requirePlain(roots.owned, true),
        proof = requirePlain(roots.proof, true);
      if (
        path.dirname(resolved) !== proof ||
        !/^nc1-[a-f0-9-]{36}$/u.test(path.basename(resolved))
      )
        throw new Error("cleanup-containment");
      for (const directory of [roots.profile, roots.project])
        if (existsSync(directory)) requirePlain(directory, true);
      remove(resolved, { recursive: true });
      return;
    } catch (error) {
      if (!error.code) throw error;
      if (
        ["EBUSY", "EPERM", "EACCES", "ENOTEMPTY"].includes(error.code) &&
        attempt < 21
      ) {
        await pause(500);
        continue;
      }
      const relative =
        typeof error.path === "string"
          ? path.relative(path.resolve(roots.owned), path.resolve(error.path))
          : "";
      const failure = new Error("roots-cleanup-failed", { cause: error });
      failure.cleanupError = {
        code: /^[A-Z0-9_]{1,32}$/u.test(error.code) ? error.code : "UNKNOWN",
        syscall: /^[a-zA-Z0-9_]{1,32}$/u.test(error.syscall)
          ? error.syscall
          : "remove",
        path:
          relative && !relative.startsWith("..") && !path.isAbsolute(relative)
            ? relative.split(path.sep).join("/").slice(0, 256)
            : "owned-root",
        attempts: attempt,
      };
      throw failure;
    }
  }
}
