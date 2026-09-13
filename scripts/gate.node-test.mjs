import assert from "node:assert/strict";
import test from "node:test";

import { environmentForGateStep } from "./gate-env.mjs";
import { stepConfiguration, steps } from "./gate-plan.mjs";

test("gate serializes Vitest by default without changing other steps", () => {
  const environment = { KEEP: "value" };

  assert.deepEqual(environmentForGateStep("vitest", environment), {
    KEEP: "value",
    VITEST_MAX_WORKERS: "1",
    VITEST_MIN_WORKERS: "1",
  });
  assert.equal(environment.VITEST_MAX_WORKERS, undefined);
  assert.equal(environment.VITEST_MIN_WORKERS, undefined);
  assert.equal(environmentForGateStep("clippy", environment), environment);
});

test("gate wiring assigns serialization only to the Vitest step", () => {
  const vitest = stepConfiguration("vitest");
  const clippy = stepConfiguration("clippy");

  // Reference equality is the revert guard: reverting the Vitest step to the
  // old array-only form cannot satisfy it.
  assert.equal(vitest, steps.vitest);
  assert.ok(!Array.isArray(steps.vitest));

  // Assert the wiring against the same derivation the gate uses, not against a
  // snapshot of the ambient environment. Hard-coding "1" here would make this
  // test — and therefore `budgets --check`, `gate:full` and `npm run check` —
  // fail whenever an operator exercises the documented worker override.
  assert.deepEqual(vitest.env, environmentForGateStep("vitest", process.env));
  assert.equal(
    vitest.env.VITEST_MAX_WORKERS,
    process.env.VITEST_MAX_WORKERS || "1",
  );
  assert.equal(
    vitest.env.VITEST_MIN_WORKERS,
    process.env.VITEST_MIN_WORKERS || "1",
  );
  assert.notEqual(vitest.env, process.env);

  assert.equal(clippy.env, process.env);
  assert.deepEqual(clippy.command, steps.clippy);
});

test("gate honors explicit Vitest worker overrides", () => {
  for (const environment of [
    { VITEST_MAX_WORKERS: "4" },
    { VITEST_MIN_WORKERS: "2" },
    { VITEST_MAX_WORKERS: "4", VITEST_MIN_WORKERS: "2" },
    { VITEST_MAX_WORKERS: "", VITEST_MIN_WORKERS: "" },
  ]) {
    const actual = environmentForGateStep("vitest", environment);
    assert.equal(
      actual.VITEST_MAX_WORKERS,
      environment.VITEST_MAX_WORKERS || "1",
    );
    assert.equal(
      actual.VITEST_MIN_WORKERS,
      environment.VITEST_MIN_WORKERS || "1",
    );
  }
});
