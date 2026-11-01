import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { mock, test } from "node:test";

import {
  failAfterTeardown,
  guardianFor,
  settleGuardian,
} from "./beta0001-installed-smoke-output-authority.mjs";

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
