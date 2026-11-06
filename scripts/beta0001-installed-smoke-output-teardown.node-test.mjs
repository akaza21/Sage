import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { spawn } from "node:child_process";
import { once } from "node:events";
import path from "node:path";
import { mock, test } from "node:test";

import {
  failAfterTeardown,
  guardianFor,
  settleGuardian,
  waitForGuardianAuthority,
} from "./beta0001-installed-smoke-output-authority.mjs";
import { observeGuardianStartup } from "./beta0001-installed-smoke-output-diagnostics.mjs";

const releaseGraceMs = 5000;
const teardownTimeoutMs = 10_000;

// A child process stand-in whose exit and close the test drives by hand.
function fakeGuardian() {
  const child = new EventEmitter();
  child.exitCode = null;
  child.signalCode = null;
  child.kills = 0;
  child.released = false;
  child.kill = () => {
    child.kills += 1;
  };
  child.stdin = new EventEmitter();
  child.stdin.end = (line) => {
    assert.equal(line, "release\n");
    child.released = true;
  };
  child.close = (code = 0) => {
    child.exitCode = code;
    child.emit("close", code);
  };
  return guardianFor(child);
}

// Lets settled promises run their reactions without advancing mocked time.
const drain = () => new Promise((resolve) => setImmediate(resolve));

async function pending(promise) {
  const marker = Symbol("pending");
  const outcome = await Promise.race([
    promise.then(
      () => "resolved",
      () => "rejected",
    ),
    drain().then(() => marker),
  ]);
  return outcome === marker;
}

test.beforeEach(() => mock.timers.enable({ apis: ["setTimeout"] }));
test.afterEach(() => mock.timers.reset());

test("an acquisition failure kills the guardian and waits for its close", async () => {
  const guardian = fakeGuardian();
  const original = new Error("installed-smoke-output-authority-proof-invalid");
  const failed = failAfterTeardown(guardian, original, { release: false });
  failed.catch(() => {});
  await drain();
  assert.equal(guardian.child.kills, 1);
  assert.equal(guardian.child.released, false);
  mock.timers.tick(teardownTimeoutMs - 1);
  assert.ok(await pending(failed), "rejected before the guardian closed");
  guardian.child.close(1);
  await assert.rejects(failed, (error) => error === original);
});

test("release waits for a delayed close without killing a guardian that exits", async () => {
  const guardian = fakeGuardian();
  const released = settleGuardian(guardian, { release: true });
  await drain();
  assert.equal(guardian.child.released, true);
  mock.timers.tick(releaseGraceMs - 1);
  assert.ok(await pending(released), "resolved before the guardian closed");
  guardian.child.close(0);
  await released;
  assert.equal(guardian.child.kills, 0);
});

test("release kills a guardian that ignores it and still waits for close", async () => {
  const guardian = fakeGuardian();
  const released = settleGuardian(guardian, { release: true });
  await drain();
  mock.timers.tick(releaseGraceMs);
  await drain();
  assert.equal(guardian.child.kills, 1);
  mock.timers.tick(teardownTimeoutMs - 1);
  assert.ok(await pending(released), "resolved before the guardian closed");
  guardian.child.close(1);
  await released;
});

test("a guardian that never closes after the kill fails teardown and keeps the original error", async () => {
  const guardian = fakeGuardian();
  const original = new Error("installed-smoke-output-authority-timeout");
  const failed = failAfterTeardown(guardian, original, { release: false });
  failed.catch(() => {});
  await drain();
  assert.equal(guardian.child.kills, 1);
  mock.timers.tick(teardownTimeoutMs);
  await assert.rejects(failed, (error) => {
    assert.ok(error instanceof AggregateError);
    assert.equal(
      error.message,
      "installed-smoke-output-authority-teardown-timeout",
    );
    assert.equal(error.cause, error.errors[0]);
    assert.equal(error.errors[1], original);
    return true;
  });
});

test("a release whose guardian never closes rejects with the teardown timeout", async () => {
  const guardian = fakeGuardian();
  const released = settleGuardian(guardian, { release: true });
  released.catch(() => {});
  await drain();
  mock.timers.tick(releaseGraceMs);
  await drain();
  mock.timers.tick(teardownTimeoutMs);
  await assert.rejects(
    released,
    /installed-smoke-output-authority-teardown-timeout/,
  );
});

test("a guardian that closed before teardown began settles at once", async () => {
  const guardian = fakeGuardian();
  guardian.child.close(0);
  await settleGuardian(guardian, { release: true });
  assert.equal(guardian.child.kills, 0);
});

test("startup timeout reports observed phases and live child state without disclosing output", async () => {
  mock.timers.reset();
  const child = spawn(
    process.execPath,
    [
      "--input-type=module",
      "-e",
      `process.stderr.write('private-path-and-environment\\nsage-output-authority-phase compile-start 12 8 3\\n');
     process.stdout.write('private-output');
     setInterval(() => {}, 1000);`,
    ],
    { stdio: ["pipe", "pipe", "pipe"], windowsHide: true },
  );
  const startup = observeGuardianStartup(child);
  const closed = once(child, "close");
  try {
    await once(child.stdout, "data");
    // stdout/stderr are independent pipes; wait for both observations.
    while (!startup.snapshot().phases["compile-start"])
      await once(child.stderr, "data");
    const powerShell =
      process.platform === "win32"
        ? {
            path: path.join(
              process.env.SystemRoot,
              "System32",
              "WindowsPowerShell",
              "v1.0",
              "powershell.exe",
            ),
          }
        : undefined;
    const error = startup.timeoutError();
    await startup.collectCpu(error, powerShell);
    assert.match(error.message, /^installed-smoke-output-authority-timeout /u);
    assert.doesNotMatch(error.message, /private|process\.stderr|setInterval/u);
    const state = error.diagnostic;
    assert.equal(state.authority, "ready-not-observed");
    assert.equal(state.child.state, "running");
    assert.equal(state.child.pid, child.pid);
    assert.equal(state.stdout.bytes, 14);
    assert.equal(state.stdout.ended, false);
    assert.deepEqual(state.phases["compile-start"], {
      elapsedMs: 12,
      userMs: 8,
      kernelMs: 3,
      observedMs: state.phases["compile-start"].observedMs,
    });
    assert.ok(state.elapsedMs >= state.phases["compile-start"].observedMs);
    assert.equal(state.phases["ready-written"], null);
    // Sampling can legitimately be unavailable under the very startup stall
    // being diagnosed. Only validate accounting when the bounded probe got it.
    assert.ok(
      ["sampled-by-pid", "unavailable"].includes(state.child.cpu.status),
    );
    if (state.child.cpu.status === "sampled-by-pid") {
      assert.ok(state.child.cpu.userMs >= 0);
      assert.ok(state.child.cpu.kernelMs >= 0);
    }
  } finally {
    startup.dispose();
    child.kill();
    await closed;
  }
});

test("authority deadline reports startup state before killing and closing the stalled child", async () => {
  const child = spawn(
    process.execPath,
    [
      "--input-type=module",
      "-e",
      `process.stderr.write('sage-output-authority-phase seal-start 15 5 2\\n');
     process.stdout.write('partial');
     setInterval(() => {}, 1000);`,
    ],
    { stdio: ["pipe", "pipe", "pipe"], windowsHide: true },
  );
  const guardian = guardianFor(child);
  // Node cannot execute PowerShell arguments: this deterministically exercises
  // unavailable accounting without depending on system PowerShell startup.
  const waiting = waitForGuardianAuthority(
    guardian,
    path.resolve("unused-output"),
    { path: process.execPath },
  );
  waiting.catch(() => {});
  try {
    await once(child.stdout, "data");
    mock.timers.tick(10_000);
    await assert.rejects(waiting, (error) => {
      assert.match(
        error.message,
        /^installed-smoke-output-authority-timeout /u,
      );
      assert.equal(error.diagnostic.child.state, "running");
      assert.equal(error.diagnostic.child.cpu.status, "unavailable");
      assert.equal(error.diagnostic.stdout.bytes, 7);
      assert.equal(error.diagnostic.stdout.ended, false);
      return true;
    });
    // Failure is not returned until the actual child has closed.
    assert.ok(child.exitCode !== null || child.signalCode !== null);
    await guardian.closed;
  } finally {
    if (child.exitCode === null && child.signalCode === null) child.kill();
    await guardian.closed;
  }
});

test("startup diagnostics bound and reassemble phase lines and retain exit and EOF observations", async () => {
  mock.timers.reset();
  const child = spawn(
    process.execPath,
    [
      "--input-type=module",
      "-e",
      `process.stderr.write('x'.repeat(20000) + 'sage-output-authority-phase seal-start 1 2 3\\n');
     process.stderr.write('sage-output-authority-phase unknown 1 2 3\\n');
     process.stderr.write('sage-output-authority-phase compile-');
     setTimeout(() => process.stderr.end('complete 30 20 4\\n'), 10);`,
    ],
    { stdio: ["pipe", "pipe", "pipe"], windowsHide: true },
  );
  const startup = observeGuardianStartup(child);
  try {
    await once(child, "close");
    const state = startup.timeoutError().diagnostic;
    assert.equal(state.child.state, "exited");
    assert.equal(state.child.exitCode, 0);
    assert.equal(state.stdout.ended, true);
    assert.equal(state.stderr.ended, true);
    assert.equal(state.phases["seal-start"], null);
    assert.equal(Object.hasOwn(state.phases, "unknown"), false);
    assert.equal(state.phases["compile-complete"].elapsedMs, 30);
    assert.equal(state.phases["compile-complete"].userMs, 20);
    assert.ok(state.events.closeMs >= state.events.exitMs);
    assert.ok(JSON.stringify(state).length < 2048);
  } finally {
    startup.dispose();
  }
});

test("a stalled CPU sampler cannot delay authority rejection or killing the guardian", async () => {
  const child = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], {
    stdio: ["pipe", "pipe", "pipe"],
    windowsHide: true,
  });
  const guardian = guardianFor(child);
  let finishSampling;
  const sampling = new Promise((resolve) => {
    finishSampling = resolve;
  });
  const waiting = waitForGuardianAuthority(
    guardian,
    path.resolve("unused-output"),
    undefined,
    { sampleCpu: () => sampling },
  );
  waiting.catch(() => {});
  try {
    await once(child, "spawn");
    mock.timers.tick(10_000);
    assert.equal(
      child.killed,
      true,
      "deadline must issue the kill synchronously",
    );
    let failure;
    await assert.rejects(waiting, (error) => {
      failure = error;
      assert.match(
        error.message,
        /^installed-smoke-output-authority-timeout /u,
      );
      return true;
    });
    assert.ok(child.exitCode !== null || child.signalCode !== null);
    assert.ok(await pending(failure.diagnosticReady));
    finishSampling({ status: "sampled-by-pid", userMs: 17, kernelMs: 4 });
    await failure.diagnosticReady;
    assert.equal(failure.diagnostic.child.cpu.userMs, 17);
    assert.equal(failure.diagnostic.child.cpu.kernelMs, 4);
    assert.deepEqual(
      JSON.parse(failure.message.slice(failure.message.indexOf(" ") + 1)),
      failure.diagnostic,
    );
  } finally {
    finishSampling({ status: "unavailable" });
    if (child.exitCode === null && child.signalCode === null) child.kill();
    await guardian.closed;
  }
});

test("sampling cannot postpone the teardown deadline for a guardian that never closes", async () => {
  const guardian = fakeGuardian();
  guardian.child.stdout = new EventEmitter();
  guardian.child.stderr = new EventEmitter();
  let finishSampling;
  const sampling = new Promise((resolve) => {
    finishSampling = resolve;
  });
  const waiting = waitForGuardianAuthority(
    guardian,
    path.resolve("unused-output"),
    undefined,
    {
      sampleCpu: () => {
        mock.timers.tick(teardownTimeoutMs);
        return sampling;
      },
    },
  );
  waiting.catch(() => {});
  try {
    mock.timers.tick(10_000);
    await assert.rejects(waiting, (error) => {
      assert.ok(error instanceof AggregateError);
      assert.equal(
        error.message,
        "installed-smoke-output-authority-teardown-timeout",
      );
      assert.match(
        error.errors[1].message,
        /^installed-smoke-output-authority-timeout /u,
      );
      return true;
    });
  } finally {
    finishSampling({ status: "unavailable" });
  }
});
