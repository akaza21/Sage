import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer as createHttpServer } from "node:http";
import { createServer } from "node:net";
import { resolve, dirname } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  killProcessTree,
  portAccepts,
  settleDevServer,
  startDevServer,
  waitForServer,
} from "./lib/app-smoke-server.mjs";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const LATE_BIND_GRACE_MS = 4_000;
const DEADLINE_SLACK_MS = 5_000;
const REAP_GRACE_MS = 1_000;

function reserveFreePort() {
  return new Promise((done, fail) => {
    const probe = createServer();
    probe.once("error", fail);
    probe.listen(0, "127.0.0.1", () => {
      const { port } = probe.address();
      probe.close(() => done(port));
    });
  });
}

function listenSilently(t) {
  const sockets = new Set();
  const silent = createServer((socket) => {
    sockets.add(socket);
    socket.once("close", () => sockets.delete(socket));
  });
  return new Promise((done) => {
    silent.listen(0, "127.0.0.1", () => {
      t.after(() => {
        for (const socket of sockets) socket.destroy();
        return new Promise((closed) => silent.close(closed));
      });
      done({ port: silent.address().port });
    });
  });
}

function listenUnready(t) {
  const unready = createHttpServer((request, response) => {
    response.statusCode = 503;
    response.end();
  });
  return new Promise((done) => {
    unready.listen(0, "127.0.0.1", () => {
      t.after(() => new Promise((closed) => unready.close(closed)));
      done({ port: unready.address().port });
    });
  });
}

function processAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error.code !== "ESRCH";
  }
}

test("a self-started dev server releases its port and process on success", async (t) => {
  const port = await reserveFreePort();
  const url = `http://127.0.0.1:${port}/`;
  const server = await startDevServer(repositoryRoot, url);
  const pid = server.child.pid;
  t.after(() => settleDevServer(server));

  assert.equal(await portAccepts("127.0.0.1", port), true);

  assert.equal(await settleDevServer(server), true);
  assert.equal(await portAccepts("127.0.0.1", port), false);
  assert.equal(processAlive(pid), false);
});

test("a dev server that never becomes ready is settled before the failure propagates", async () => {
  const port = await reserveFreePort();
  const url = `http://127.0.0.1:${port}/`;
  await assert.rejects(
    startDevServer(repositoryRoot, url, { readyTimeoutMs: 1 }),
    (error) => {
      assert.match(error.message, /did not start within/u);
      assert.equal(typeof error.devServerPid, "number");
      assert.equal(error.settled, true);
      assert.equal(processAlive(error.devServerPid), false);
      return true;
    },
  );
  await new Promise((done) => setTimeout(done, LATE_BIND_GRACE_MS));
  assert.equal(await portAccepts("127.0.0.1", port), false);
});

test("settling a run that owns no dev server terminates nothing", async () => {
  assert.equal(await settleDevServer(undefined), true);
  assert.equal(await settleDevServer({ child: undefined }), true);
});

test(
  "waiting on a port that accepts but never answers still fails by the deadline",
  { timeout: 30_000 },
  async (t) => {
    const silent = await listenSilently(t);
    const timeoutMs = 3_000;
    const started = Date.now();
    await assert.rejects(
      waitForServer(`http://127.0.0.1:${silent.port}/`, timeoutMs),
      /did not start within/u,
    );
    assert.ok(Date.now() - started < timeoutMs + DEADLINE_SLACK_MS);
  },
);

test(
  "settling a dev server that never exits reports failure",
  { timeout: 60_000 },
  async () => {
    const port = await reserveFreePort();
    const stubborn = {
      exitCode: null,
      signalCode: null,
      kill() {},
      once() {},
    };
    const server = { child: stubborn, url: `http://127.0.0.1:${port}/` };
    assert.equal(await settleDevServer(server), false);
    assert.equal(server.child, stubborn);
    assert.equal(await settleDevServer(server), false);
  },
);

test(
  "a dev server that cannot bind reports its own output with the failure",
  { timeout: 60_000 },
  async (t) => {
    const blocker = await listenUnready(t);
    await assert.rejects(
      startDevServer(repositoryRoot, `http://127.0.0.1:${blocker.port}/`, {
        readyTimeoutMs: 8_000,
      }),
      /already in use/u,
    );
  },
);

test(
  "settling a dev server reaps the descendants it owns",
  { timeout: 60_000 },
  async (t) => {
    const port = await reserveFreePort();
    const parent = spawn(
      process.execPath,
      [
        "-e",
        "const { spawn } = require('node:child_process');" +
          "const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });" +
          "console.log(child.pid);" +
          "setInterval(() => {}, 1000);",
      ],
      {
        stdio: ["ignore", "pipe", "ignore"],
        windowsHide: true,
        detached: process.platform !== "win32",
      },
    );
    t.after(() => killProcessTree(parent.pid));
    const descendant = await new Promise((done) => {
      parent.stdout.setEncoding("utf8");
      parent.stdout.once("data", (chunk) => done(Number(chunk.trim())));
    });

    assert.equal(processAlive(descendant), true);
    assert.equal(
      await settleDevServer({
        child: parent,
        url: `http://127.0.0.1:${port}/`,
      }),
      true,
    );
    await new Promise((done) => setTimeout(done, REAP_GRACE_MS));

    assert.equal(processAlive(parent.pid), false);
    assert.equal(processAlive(descendant), false);
  },
);
