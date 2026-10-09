import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import path from "node:path";
import { readBoundedBytes } from "./capture-native-chat-contract.mjs";
import {
  runTool,
  requirePlain,
  reservePort,
} from "./capture-native-chat-process.mjs";

export async function cleanSource(workspace, tool = runTool) {
  const root = await tool("git", [
    "-C",
    workspace,
    "rev-parse",
    "--show-toplevel",
  ]);
  if (path.relative(workspace, path.resolve(root)) !== "")
    throw new Error("source-root");
  const status = await tool("git", [
    "-C",
    workspace,
    "status",
    "--porcelain=v1",
    "--untracked-files=all",
  ]);
  if (status !== "") throw new Error("source-dirty");
  const source = await tool("git", ["-C", workspace, "rev-parse", "HEAD"]);
  if (!/^[a-f0-9]{40,64}$/u.test(source)) throw new Error("source-identity");
  return source;
}
export async function verifySource(workspace, expected, tool = runTool) {
  if ((await cleanSource(workspace, tool)) !== expected)
    throw new Error("source-drift");
}
export function viteLaunch(workspace) {
  return {
    executable: process.execPath,
    args: [
      path.join(workspace, "node_modules/vite/bin/vite.js"),
      workspace,
      "--config",
      path.join(workspace, "vite.config.ts"),
      "--host",
      "127.0.0.1",
      "--port",
      "1420",
      "--strictPort",
      "--mode",
      "desktop-native",
      "--clearScreen",
      "false",
    ],
    options: {
      cwd: workspace,
      windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env, NO_COLOR: "1" },
    },
  };
}
export async function launchOwnedVite(workspace) {
  requirePlain(workspace, true);
  const launch = viteLaunch(workspace);
  requirePlain(launch.args[0]);
  requirePlain(path.join(workspace, "vite.config.ts"));
  try {
    await reservePort(1420);
  } catch {
    throw new Error("vite-port-occupied");
  }
  return spawn(launch.executable, launch.args, launch.options);
}
export function assertOwnedVite(child) {
  if (!child.pid || child.exitCode !== null || child.signalCode !== null)
    throw new Error("owned-vite-exited");
}
export function waitForVite(child, timeoutMs = 10000) {
  return new Promise((resolve, reject) => {
    let stdout = "",
      bytes = 0;
    const finish = (error) => {
      clearTimeout(timer);
      child.off("error", onError);
      child.off("exit", onExit);
      child.stdout.off("data", onStdout);
      child.stderr.off("data", onStderr);
      if (error) reject(error);
      else resolve();
    };
    const onError = () => finish(new Error("owned-vite-blocked"));
    const onExit = () => finish(new Error("owned-vite-exited"));
    const charge = (chunk) => {
      bytes += Buffer.byteLength(chunk);
      if (bytes <= 65536) return true;
      finish(new Error("owned-vite-output-bound"));
      return false;
    };
    const onStdout = (chunk) => {
      if (!charge(chunk)) return;
      stdout += chunk.toString();
      if (stdout.includes("http://127.0.0.1:1420/")) {
        try {
          assertOwnedVite(child);
          finish();
        } catch (error) {
          finish(error);
        }
      }
    };
    const onStderr = (chunk) => {
      charge(chunk);
    };
    const timer = setTimeout(
      () => finish(new Error("owned-vite-timeout")),
      timeoutMs,
    );
    child.once("error", onError);
    child.once("exit", onExit);
    child.stdout.on("data", onStdout);
    child.stderr.on("data", onStderr);
    try {
      assertOwnedVite(child);
    } catch (error) {
      finish(error);
    }
  });
}
export async function servedAssetHash(fetcher = fetch) {
  const response = await fetcher("http://127.0.0.1:1420/src/main.tsx", {
    signal: AbortSignal.timeout(3000),
  });
  if (!response.ok) throw new Error("served-asset-status");
  const body = await readBoundedBytes(response);
  if (!body.length) throw new Error("served-asset-empty");
  return createHash("sha256").update(body).digest("hex");
}
