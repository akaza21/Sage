import path from "node:path";
import { discoverOwnedPage } from "./capture-native-chat-contract.mjs";
import { runTool } from "./capture-native-chat-process.mjs";
import { connectCdp } from "./source-functional-native-ide-cdp.mjs";

export function watchOwnedApp(child) {
  let bytes = 0;
  let tail = "";
  let reason = "exit-status-only";
  let launchCode = null;
  let rejectActive = null;
  let closed = false;
  let drain = null;
  const markClosed = () => {
    closed = true;
  };
  child.on("close", markClosed);
  const consume = (chunk) => {
    bytes = Math.min(65537, bytes + Buffer.byteLength(chunk));
    if (bytes > 65536) {
      rejectActive?.(new Error("owned-app-output-bound"));
      return;
    }
    tail = (tail + chunk.toString()).slice(-2048);
    for (const [text, slug] of [
      [
        "curated harness metadata must satisfy its transport contract",
        "harness-transport-contract",
      ],
      ["disposable profile is invalid", "fixture-profile-rejected"],
      ["fixture admission failed", "fixture-admission-failed"],
      ["failed to build Sage", "app-setup-failed"],
    ])
      if (tail.includes(text) && reason === "exit-status-only") reason = slug;
  };
  const onError = (error) => {
    launchCode = /^[A-Z0-9_]{1,32}$/u.test(error.code) ? error.code : "UNKNOWN";
  };
  child.stdout.on("data", consume);
  child.stderr.on("data", consume);
  child.on("error", onError);
  const failure = () => {
    if (bytes > 65536) return new Error("owned-app-output-bound");
    if (launchCode)
      return Object.assign(new Error("owned-app-launch-failed"), {
        appExit: {
          code: null,
          signal: null,
          reason: "launch-failed",
          launchCode,
        },
      });
    if (child.exitCode === null && child.signalCode === null && child.pid)
      return null;
    return Object.assign(new Error("owned-app-exited"), {
      appExit: { code: child.exitCode, signal: child.signalCode, reason },
    });
  };
  const drainExitOutput = () => {
    if (
      closed ||
      (!launchCode && child.exitCode === null && child.signalCode === null)
    )
      return Promise.resolve();
    drain ??= new Promise((resolve) => {
      const finish = () => {
        clearTimeout(timer);
        child.off("close", finish);
        resolve();
      };
      const timer = setTimeout(finish, 1000);
      child.once("close", finish);
    });
    return drain;
  };
  return {
    failure,
    async run(action) {
      const prior = failure();
      if (prior) {
        await drainExitOutput();
        throw failure() ?? prior;
      }
      if (rejectActive) throw new Error("owned-app-operation-overlap");
      let rejectExit;
      const exited = new Promise((_, reject) => {
        rejectExit = reject;
      });
      rejectActive = rejectExit;
      const onExit = () => {
        void drainExitOutput().then(() =>
          rejectExit(failure() ?? new Error("owned-app-exited")),
        );
      };
      child.once("exit", onExit);
      child.once("error", onExit);
      try {
        const result = await Promise.race([
          Promise.resolve().then(action),
          exited,
        ]);
        const final = failure();
        if (final) throw final;
        return result;
      } catch (error) {
        await drainExitOutput();
        throw failure() ?? error;
      } finally {
        rejectActive = null;
        child.off("exit", onExit);
        child.off("error", onExit);
      }
    },
    dispose() {
      child.stdout.off("data", consume);
      child.stderr.off("data", consume);
      child.off("error", onError);
      child.off("close", markClosed);
    },
  };
}

export async function openOwnedCapture(
  child,
  port,
  deadline,
  options,
  workspace,
  record,
) {
  const monitor = watchOwnedApp(child);
  try {
    const target = await monitor.run(() =>
      discoverOwnedPage(port, deadline, child),
    );
    record("cdp-target-ready");
    const bounds = JSON.parse(
      await monitor.run(() =>
        runTool("powershell.exe", [
          "-NoProfile",
          "-NonInteractive",
          "-File",
          path.join(workspace, "scripts", "capture-native-chat-window.ps1"),
          "-ProcessId",
          String(child.pid),
          "-Width",
          String(options.width),
          "-Port",
          String(port),
        ]),
      ),
    );
    const connection = await monitor.run(() =>
      connectCdp({
        target,
        deadline,
        openDeadline: Date.now() + 10000,
        WebSocketImpl: globalThis.WebSocket,
      }),
    );
    return {
      cdp: {
        ...connection,
        evaluate: (...args) => monitor.run(() => connection.evaluate(...args)),
        screenshot: (...args) =>
          monitor.run(() => connection.screenshot(...args)),
        mouse: (...args) => monitor.run(() => connection.mouse(...args)),
        key: (...args) => monitor.run(() => connection.key(...args)),
      },
      nativeBounds: { outer: bounds.outer, client: bounds.client },
      webviewVersion: bounds.webviewVersion,
      dispose: monitor.dispose,
    };
  } catch (error) {
    monitor.dispose();
    throw error;
  }
}
