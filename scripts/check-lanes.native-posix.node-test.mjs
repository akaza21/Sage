import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { Writable } from "node:stream";
import test from "node:test";

import { acquireCargoTargetLease } from "./lib/check-cargo-lease.mjs";
import { executeCommand } from "./lib/check-process.mjs";
// origin: check-lanes.node-test.mjs:662 "case variants contend across target materialization on case-insensitive hosts" -> check-lanes.native-posix.node-test.mjs (Darwin arm, named Darwin run, without a Linux-only gate)
// Host capability: requires a case-insensitive filesystem (Darwin default).
// On a case-sensitive host the second lease takes a different lock file and
// the contention assertion fails closed; that failure is the gate working,
// never a pass.
test("case variants contend across target materialization on case-insensitive hosts [native-POSIX/Darwin]", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-case-darwin-"));
  let lease;
  try {
    lease = await acquireCargoTargetLease(path.join(root, "Target", "New"), {
      repositoryRoot: root,
      platform: "darwin",
    });
    await assert.rejects(
      acquireCargoTargetLease(path.join(root, "target", "new"), {
        repositoryRoot: root,
        platform: "darwin",
      }),
      (error) => error?.code === "cargo-target-locked",
    );
  } finally {
    await lease?.release();
    await rm(root, { recursive: true, force: true });
  }
});

// origin: check-lanes.node-test.mjs:1504 "POSIX timeout escalates the process group after its parent exits on TERM" -> check-lanes.native-posix.node-test.mjs
test("POSIX timeout escalates the process group after its parent exits on TERM", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-posix-group-"));
  const descendantPidPath = path.join(root, "descendant.pid");
  const descendantSource =
    "process.on('SIGTERM', () => {}); setInterval(() => {}, 1000)";
  const parentSource = `
      const { spawn } = require("node:child_process");
      const { writeFileSync } = require("node:fs");
      const descendant = spawn(process.execPath, ["-e", ${JSON.stringify(descendantSource)}], {
        stdio: "ignore",
      });
      writeFileSync(process.argv[1], String(descendant.pid));
      process.on("SIGTERM", () => process.exit(0));
      setInterval(() => {}, 1000);
    `;
  let descendantPid;
  try {
    await assert.rejects(
      executeCommand(
        {
          label: "posix-group-escalation",
          command: process.execPath,
          args: ["-e", parentSource, descendantPidPath],
          cwd: root,
          timeoutMs: 300,
          terminationGraceMs: 200,
          settlementTimeoutMs: 1_500,
          maximumLineBytes: 1_024,
          maximumOutputBytes: 4_096,
        },
        new AbortController().signal,
        { stdout: discardStream(), stderr: discardStream() },
      ),
      (error) => error?.code === "command-timeout" && error.settled === true,
    );
    descendantPid = Number.parseInt(
      await readFile(descendantPidPath, "utf8"),
      10,
    );
    await waitForProcessExit(descendantPid);
  } finally {
    if (descendantPid && (await processExists(descendantPid))) {
      process.kill(descendantPid, "SIGKILL");
    }
    await rm(root, { recursive: true, force: true });
  }
});

function discardStream() {
  return new Writable({
    write(_chunk, _encoding, callback) {
      callback();
    },
  });
}

async function processExists(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    if (error.code === "ESRCH") return false;
    throw error;
  }
}

async function waitForProcessExit(pid, timeoutMs = 2_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (!(await processExists(pid))) return;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  assert.fail(`process ${pid} remained alive after the runner settled`);
}
