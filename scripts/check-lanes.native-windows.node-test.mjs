import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { Writable } from "node:stream";
import test from "node:test";

import {
  acquireCargoTargetLease,
  cargoLeasePath,
} from "./lib/check-cargo-lease.mjs";
import { runCommandGroups } from "./lib/check-process.mjs";
import { executeWindowsCargoJobCommand } from "./lib/check-windows-cargo-job.mjs";
// origin: check-lanes.node-test.mjs:314 "Windows Cargo Job preserves Cargo proxy command semantics" -> check-lanes.native-windows.node-test.mjs
test("Windows Cargo Job preserves Cargo proxy command semantics", async () => {
  await executeWindowsCargoJobCommand(
    {
      label: "cargo-locate-project",
      command: "cargo.exe",
      args: ["locate-project", "--manifest-path", "src-tauri/Cargo.toml"],
      cwd: path.resolve("."),
      timeoutMs: 10_000,
      terminationGraceMs: 500,
      settlementTimeoutMs: 2_000,
      maximumLineBytes: 1_024,
      maximumOutputBytes: 4_096,
    },
    new AbortController().signal,
    { stdout: discardStream(), stderr: discardStream() },
  );
});

// origin: check-lanes.node-test.mjs:815 "Windows Cargo nonzero waits for descendant zero and releases its lease" -> check-lanes.native-windows.node-test.mjs
test("Windows Cargo nonzero waits for descendant zero and releases its lease", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-failure-"));
  const target = path.join(root, "target");
  const command = {
    label: "natural-nonzero",
    command: process.execPath,
    args: ["-e", "process.exit(7)"],
    cwd: root,
    timeoutMs: 2_000,
    terminationGraceMs: 200,
    settlementTimeoutMs: 3_000,
    maximumLineBytes: 1_024,
    maximumOutputBytes: 4_096,
  };
  try {
    const descendantPidPath = path.join(root, "nonzero-descendant.pid");
    command.args = [
      "-e",
      `const {spawn}=require("node:child_process");const {writeFileSync}=require("node:fs");const d=spawn(process.execPath,["-e","setTimeout(()=>{},300)"],{stdio:"ignore"});writeFileSync(process.argv[1],String(d.pid));process.exit(7)`,
      descendantPidPath,
    ];
    await assert.rejects(
      runCommandGroups([
        {
          label: "rust-natural-failure",
          concurrency: 1,
          timeoutMs: 8_000,
          cargoTargetDir: target,
          repositoryRoot: root,
          commands: [command],
        },
      ]),
      (error) => error?.code === "command-failed" && error.settled === true,
    );
    const descendantPid = Number.parseInt(
      await readFile(descendantPidPath, "utf8"),
      10,
    );
    assert.equal(await processExists(descendantPid), false);
    await assert.rejects(
      readFile(await cargoLeasePath(target, { repositoryRoot: root })),
      /ENOENT/u,
    );
    const retry = (cargoTargetDir) =>
      runCommandGroups([
        {
          label: "rust-immediate-retry",
          concurrency: 1,
          timeoutMs: 6_000,
          cargoTargetDir,
          repositoryRoot: root,
          commands: [
            {
              ...command,
              label: "immediate-success",
              args: ["-e", "process.exit(0)"],
            },
          ],
        },
      ]);
    await retry(target);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// origin: check-lanes.node-test.mjs:924 "Windows Cargo success waits for an owned descendant tree to reach zero" -> check-lanes.native-windows.node-test.mjs
test("Windows Cargo success waits for an owned descendant tree to reach zero", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-job-success-"));
  const descendantPidPath = path.join(root, "descendant.pid");
  const descendantDonePath = path.join(root, "descendant.done");
  const source = `
      const { spawn } = require("node:child_process");
      const { writeFileSync } = require("node:fs");
      const descendant = spawn(process.execPath, ["-e", "require('node:fs').writeFileSync(process.argv[1], 'done')", ${JSON.stringify(descendantDonePath)}], {
        detached: true,
        stdio: "ignore",
        windowsHide: true,
      });
      writeFileSync(process.argv[1], String(descendant.pid));
      process.exit(0);
    `;
  let descendantPid;
  const startedAt = Date.now();
  try {
    await runCommandGroups([
      {
        label: "rust-owned-success",
        concurrency: 1,
        timeoutMs: 10_000,
        cargoTargetDir: path.join(root, "target"),
        repositoryRoot: root,
        commands: [
          {
            label: "owned-success",
            command: process.execPath,
            args: ["-e", source, descendantPidPath],
            cwd: root,
            timeoutMs: 8_000,
            terminationGraceMs: 200,
            settlementTimeoutMs: 2_000,
            maximumLineBytes: 1_024,
            maximumOutputBytes: 4_096,
          },
        ],
      },
    ]);
    descendantPid = Number.parseInt(
      await readFile(descendantPidPath, "utf8"),
      10,
    );
    assert.ok(Date.now() - startedAt >= 300);
    assert.equal(await processExists(descendantPid), false);
    assert.equal(await readFile(descendantDonePath, "utf8"), "done");
    await assert.rejects(
      readFile(
        await cargoLeasePath(path.join(root, "target"), {
          repositoryRoot: root,
        }),
      ),
      /ENOENT/u,
    );
  } finally {
    await cleanProcessFixture(root, descendantPidPath, descendantPid);
  }
});

// origin: check-lanes.node-test.mjs:988 "Windows Cargo Job preserves argv cwd environment and bounded output" -> check-lanes.native-windows.node-test.mjs
test("Windows Cargo Job preserves argv cwd environment and bounded output", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-job-io-"));
  const values = ["", "with space", 'quote"inside', "trailing\\"];
  let output = "";
  try {
    await runCommandGroups(
      [
        {
          label: "rust-owned-io",
          concurrency: 1,
          timeoutMs: 5_000,
          cargoTargetDir: path.join(root, "target"),
          repositoryRoot: root,
          commands: [
            {
              label: "owned-io",
              command: process.execPath,
              args: [
                "-e",
                "process.stdout.write(JSON.stringify({argv:process.argv.slice(1),cwd:process.cwd(),env:process.env.SAGE_JOB_TEST}))",
                ...values,
              ],
              cwd: root,
              env: { SAGE_JOB_TEST: "exact-value" },
              timeoutMs: 3_000,
              terminationGraceMs: 200,
              settlementTimeoutMs: 1_000,
              maximumLineBytes: 2_048,
              maximumOutputBytes: 4_096,
            },
          ],
        },
      ],
      {
        executeWindowsCargo: (command, signal, options) =>
          executeWindowsCargoJobCommand(command, signal, {
            ...options,
            stdout: new Writable({
              write(chunk, _encoding, callback) {
                output += chunk.toString();
                callback();
              },
            }),
            stderr: discardStream(),
          }),
      },
    );
    const receipt = JSON.parse(output.replace(/^\[owned-io\] /u, ""));
    assert.deepEqual(receipt.argv, values);
    assert.equal(receipt.cwd, root);
    assert.equal(receipt.env, "exact-value");
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// origin: check-lanes.node-test.mjs:1048 "Windows Cargo Job streams before completion and honors sink backpressure" -> check-lanes.native-windows.node-test.mjs
test("Windows Cargo Job streams before completion and honors sink backpressure", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-job-live-"));
  let output = "";
  let firstDelivered;
  const first = new Promise((resolve) => {
    firstDelivered = resolve;
  });
  const sink = new Writable({
    highWaterMark: 1,
    write(chunk, _encoding, callback) {
      setTimeout(() => {
        output += chunk.toString();
        if (output.includes("first\n")) firstDelivered();
        callback();
      }, 75);
    },
  });
  let settled = false;
  try {
    const execution = executeWindowsCargoJobCommand(
      {
        label: "owned-live",
        command: process.execPath,
        args: [
          "-e",
          "process.stdout.write('first\\n');setTimeout(()=>process.stdout.write('second\\n'),750)",
        ],
        cwd: root,
        timeoutMs: 8_000,
        terminationGraceMs: 500,
        settlementTimeoutMs: 2_000,
        maximumLineBytes: 1_024,
        maximumOutputBytes: 4_096,
      },
      new AbortController().signal,
      { stdout: sink, stderr: discardStream() },
    );
    execution.then(
      () => {
        settled = true;
      },
      () => {
        settled = true;
      },
    );
    const firstTimer = setTimeout(
      () => firstDelivered(new Error("live output was not delivered")),
      6_000,
    );
    const delivery = await first;
    clearTimeout(firstTimer);
    if (delivery instanceof Error) throw delivery;
    assert.equal(settled, false);
    await execution;
    assert.match(output, /\[owned-live\] first\n\[owned-live\] second\n/u);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// origin: check-lanes.node-test.mjs:1112 "Windows Cargo Job streams both outputs near their independent caps" -> check-lanes.native-windows.node-test.mjs
test("Windows Cargo Job streams both outputs near their independent caps", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-job-dual-"));
  let stdout = "";
  let stderr = "";
  const capture = (append) =>
    new Writable({
      write(chunk, _encoding, callback) {
        append(chunk.toString());
        callback();
      },
    });
  try {
    await executeWindowsCargoJobCommand(
      {
        label: "owned-dual",
        command: process.execPath,
        args: [
          "-e",
          "process.stdout.write('A'.repeat(60000));process.stderr.write('B'.repeat(60000))",
        ],
        cwd: root,
        timeoutMs: 8_000,
        terminationGraceMs: 500,
        settlementTimeoutMs: 2_000,
        maximumLineBytes: 65_536,
        maximumOutputBytes: 65_536,
      },
      new AbortController().signal,
      {
        stdout: capture((value) => {
          stdout += value;
        }),
        stderr: capture((value) => {
          stderr += value;
        }),
      },
    );
    assert.equal(stdout.match(/A/gu)?.length, 60_000);
    assert.equal(stderr.match(/B/gu)?.length, 60_000);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// origin: check-lanes.node-test.mjs:1160 "Windows Cargo Job emits a truncation marker and observes grace before force" -> check-lanes.native-windows.node-test.mjs
test("Windows Cargo Job emits a truncation marker and observes grace before force", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-job-grace-"));
  const donePath = path.join(root, "done");
  let output = "";
  try {
    await assert.rejects(
      executeWindowsCargoJobCommand(
        {
          label: "owned-grace",
          command: process.execPath,
          args: [
            "-e",
            `const {writeFileSync}=require("node:fs");process.stdout.write("x".repeat(128));setTimeout(()=>writeFileSync(process.argv[1],"done"),250);setTimeout(()=>process.exit(0),350)`,
            donePath,
          ],
          cwd: root,
          timeoutMs: 8_000,
          terminationGraceMs: 1_000,
          settlementTimeoutMs: 2_000,
          maximumLineBytes: 32,
          maximumOutputBytes: 1_024,
        },
        new AbortController().signal,
        {
          stdout: new Writable({
            write(chunk, _encoding, callback) {
              output += chunk.toString();
              callback();
            },
          }),
          stderr: discardStream(),
        },
      ),
      (error) => error?.code === "output-overflow" && error.settled === true,
    );
    assert.equal(await readFile(donePath, "utf8"), "done");
    assert.equal(output.match(/x/gu)?.length, 32);
    assert.match(output, /\u2026\[truncated\]\n/u);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// origin: check-lanes.node-test.mjs:1207 "Windows Cargo Job timeout proves descendant zero" -> check-lanes.native-windows.node-test.mjs
test("Windows Cargo Job timeout proves descendant zero", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-job-timeout-"));
  const pidPath = path.join(root, "descendant.pid");
  const target = path.join(root, "target");
  const source = `const {spawn}=require("node:child_process");const {writeFileSync}=require("node:fs");const d=spawn(process.execPath,["-e","setInterval(()=>{},1000)"],{stdio:"ignore"});writeFileSync(process.argv[1],String(d.pid));setInterval(()=>{},1000)`;
  let descendantPid;
  try {
    await assert.rejects(
      runCommandGroups([
        {
          label: "rust-owned-timeout",
          concurrency: 1,
          timeoutMs: 8_000,
          cargoTargetDir: target,
          repositoryRoot: root,
          commands: [
            {
              label: "owned-timeout",
              command: process.execPath,
              args: ["-e", source, pidPath],
              cwd: root,
              timeoutMs: 1_500,
              terminationGraceMs: 500,
              settlementTimeoutMs: 3_000,
              maximumLineBytes: 1_024,
              maximumOutputBytes: 4_096,
            },
          ],
        },
      ]),
      (error) => error?.code === "command-timeout",
    );
    descendantPid = Number.parseInt(await readFile(pidPath, "utf8"), 10);
    assert.equal(await processExists(descendantPid), false);
    await assert.rejects(
      readFile(await cargoLeasePath(target, { repositoryRoot: root })),
      /ENOENT/u,
    );
    const lease = await acquireCargoTargetLease(target, {
      repositoryRoot: root,
    });
    await lease.release();
  } finally {
    await cleanProcessFixture(root, pidPath, descendantPid);
  }
});

// origin: check-lanes.node-test.mjs:1258 "Windows Cargo Job abort and output overflow prove tree zero" -> check-lanes.native-windows.node-test.mjs
test("Windows Cargo Job abort and output overflow prove tree zero", async () => {
  for (const scenario of ["abort", "overflow"]) {
    const root = await mkdtemp(
      path.join(tmpdir(), `sage-check-job-${scenario}-`),
    );
    const pidPath = path.join(root, "descendant.pid");
    const source = `const {spawn}=require("node:child_process");const {writeFileSync}=require("node:fs");const d=spawn(process.execPath,["-e","setInterval(()=>{},1000)"],{stdio:"ignore"});writeFileSync(process.argv[1],String(d.pid));${
      scenario === "overflow"
        ? "setInterval(()=>process.stdout.write('x'.repeat(2048)),20)"
        : "setInterval(()=>{},1000)"
    }`;
    const controller = new AbortController();
    if (scenario === "abort") {
      setTimeout(() => controller.abort(new Error("test abort")), 1_500);
    }
    let descendantPid;
    try {
      await assert.rejects(
        executeWindowsCargoJobCommand(
          {
            label: `owned-${scenario}`,
            command: process.execPath,
            args: ["-e", source, pidPath],
            cwd: root,
            timeoutMs: 5_000,
            terminationGraceMs: 200,
            settlementTimeoutMs: 1_000,
            maximumLineBytes: scenario === "overflow" ? 1_024 : 4_096,
            maximumOutputBytes: 4_096,
          },
          controller.signal,
          {
            repositoryRoot: root,
            stdout: discardStream(),
            stderr: discardStream(),
          },
        ),
        (error) =>
          error?.code ===
            (scenario === "abort" ? "command-aborted" : "output-overflow") &&
          error.settled === true,
      );
      descendantPid = Number.parseInt(await readFile(pidPath, "utf8"), 10);
      assert.equal(await processExists(descendantPid), false);
    } finally {
      await cleanProcessFixture(root, pidPath, descendantPid);
    }
  }
});

// origin: check-lanes.node-test.mjs:662 "case variants contend across target materialization on case-insensitive hosts" -> check-lanes.native-windows.node-test.mjs (Windows arm, win32 lease-identity folding on NTFS)
// Host capability: requires native Windows with a case-insensitive (NTFS)
// host path. Off-host the contention assertion fails closed, never passes.
test("case variants contend across target materialization on case-insensitive hosts [native-Windows/NTFS]", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-case-windows-"));
  let lease;
  try {
    lease = await acquireCargoTargetLease(path.join(root, "Target", "New"), {
      repositoryRoot: root,
      platform: "win32",
    });
    await assert.rejects(
      acquireCargoTargetLease(path.join(root, "target", "new"), {
        repositoryRoot: root,
        platform: "win32",
      }),
      (error) => error?.code === "cargo-target-locked",
    );
  } finally {
    await lease?.release();
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

async function cleanProcessFixture(root, pidPath, descendantPid) {
  if (!descendantPid) {
    try {
      const candidate = Number.parseInt(await readFile(pidPath, "utf8"), 10);
      if (Number.isSafeInteger(candidate) && candidate > 0) {
        descendantPid = candidate;
      }
    } catch (error) {
      if (error?.code !== "ENOENT") throw error;
    }
  }
  if (descendantPid && (await processExists(descendantPid))) {
    try {
      process.kill(descendantPid, "SIGKILL");
    } catch (error) {
      if (error?.code !== "ESRCH") throw error;
    }
    await waitForProcessExit(descendantPid);
  }
  await rm(root, {
    recursive: true,
    force: true,
    maxRetries: 5,
    retryDelay: 100,
  });
}
