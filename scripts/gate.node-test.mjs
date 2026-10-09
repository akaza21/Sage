import assert from "node:assert/strict";
import test from "node:test";

import { environmentForGateStep } from "./gate-env.mjs";
import { plans, stepConfiguration, steps } from "./gate-plan.mjs";

test("Rust gates rebuild the fixture before the existing test phases", () => {
  for (const mode of ["rust", "full"]) {
    const plan = plans[mode];
    const buildIndex = plan.indexOf("rust-fixture");
    assert.ok(buildIndex >= 0, `${mode} must rebuild the fixture`);
    assert.equal(plan.filter((name) => name === "rust-fixture").length, 1);
    assert.deepEqual(plan.slice(buildIndex, buildIndex + 4), [
      "rust-fixture",
      "rust-parallel",
      "rust-git-proc",
      "rust-preview",
    ]);
  }
  for (const mode of ["static", "fast"]) {
    assert.ok(!plans[mode].includes("rust-fixture"));
  }
});

test("fixture build shares the Rust tests' features and target environment", () => {
  const fixture = stepConfiguration("rust-fixture");
  assert.deepEqual(fixture.command, [
    "cargo",
    "build",
    "--manifest-path",
    "src-tauri/Cargo.toml",
    "--all-features",
    "--bin",
    "acp-adversarial-fixture",
  ]);
  assert.equal(fixture.env, process.env);
  for (const name of ["rust-parallel", "rust-git-proc", "rust-preview"]) {
    const rust = stepConfiguration(name);
    assert.equal(rust.env, fixture.env);
    assert.deepEqual(rust.command.slice(2, 5), fixture.command.slice(2, 5));
  }
});

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
