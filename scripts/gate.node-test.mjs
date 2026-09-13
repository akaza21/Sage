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

  assert.equal(vitest, steps.vitest);
  assert.equal(vitest.env.VITEST_MAX_WORKERS, "1");
  assert.equal(vitest.env.VITEST_MIN_WORKERS, "1");
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
