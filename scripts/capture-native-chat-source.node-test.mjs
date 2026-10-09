import assert from "node:assert/strict";
import test from "node:test";
import path from "node:path";
import { EventEmitter } from "node:events";
import { createHash } from "node:crypto";
import {
  cleanSource,
  verifySource,
  viteLaunch,
  waitForVite,
  servedAssetHash,
} from "./capture-native-chat-source.mjs";

const workspace = path.resolve("synthetic-checkout");
const source = "a".repeat(40);
test("capture requires clean source from its exact checkout and stable HEAD", async () => {
  const tool =
    (status = "", head = source, root = workspace) =>
    async (_, args) => {
      assert.deepEqual(args.slice(0, 2), ["-C", workspace]);
      if (args[2] === "status") {
        assert.deepEqual(args.slice(3), [
          "--porcelain=v1",
          "--untracked-files=all",
        ]);
        return status;
      }
      return args[3] === "--show-toplevel" ? root : head;
    };
  assert.equal(await cleanSource(workspace, tool()), source);
  for (const status of [" M src/main.tsx", "?? src/new.ts", "A  src/main.tsx"])
    await assert.rejects(cleanSource(workspace, tool(status)), /source-dirty/u);
  await assert.rejects(
    cleanSource(workspace, tool("", "wrong")),
    /source-identity/u,
  );
  await assert.rejects(
    cleanSource(workspace, tool("", source, path.resolve("other"))),
    /source-root/u,
  );
  await assert.rejects(
    verifySource(workspace, source, tool("", "b".repeat(40))),
    /source-drift/u,
  );
});
test("owned Vite launch pins checkout root, config, mode and strict port", () => {
  const launch = viteLaunch(workspace);
  assert.equal(launch.executable, process.execPath);
  assert.deepEqual(launch.args, [
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
  ]);
  assert.equal(launch.options.cwd, workspace);
  assert.equal(launch.options.windowsHide, true);
});
const child = () =>
  Object.assign(new EventEmitter(), {
    pid: 1,
    exitCode: null,
    signalCode: null,
    stdout: new EventEmitter(),
    stderr: new EventEmitter(),
  });
test("readiness belongs to owned stdout; timeout, early exit and bounds fail closed", async () => {
  const ready = child();
  const waiting = waitForVite(ready, 1000);
  ready.stdout.emit("data", "Local: http://127.0.0.1:1420/");
  await waiting;
  for (const trigger of [
    (candidate) => candidate.emit("exit", 1),
    (candidate) => candidate.emit("error", new Error("private")),
    (candidate) => candidate.stdout.emit("data", Buffer.alloc(65537)),
    (candidate) => candidate.stderr.emit("data", "http://127.0.0.1:1420/"),
  ]) {
    const candidate = child();
    const pending = waitForVite(candidate, 5);
    trigger(candidate);
    await assert.rejects(
      pending,
      /owned-vite-(exited|blocked|output-bound|timeout)/u,
    );
  }
});
test("served asset hashes exact bounded body and rejects status and oversized responses", async () => {
  const body = "export const synthetic = 1;";
  const fetcher = async (url, options) => {
    assert.equal(url, "http://127.0.0.1:1420/src/main.tsx");
    assert.ok(options.signal);
    return new Response(body);
  };
  assert.equal(
    await servedAssetHash(fetcher),
    createHash("sha256").update(body).digest("hex"),
  );
  const raw = Buffer.from([0xef, 0xbb, 0xbf, 0xff]);
  assert.equal(
    await servedAssetHash(async () => new Response(raw)),
    createHash("sha256").update(raw).digest("hex"),
  );
  await assert.rejects(
    servedAssetHash(async () => new Response(body, { status: 404 })),
    /served-asset-status/u,
  );
  await assert.rejects(
    servedAssetHash(async () => new Response("x".repeat(65537))),
    /cdp-discovery-bound/u,
  );
});
