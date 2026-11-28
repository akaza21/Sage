import assert from "node:assert/strict";
import test from "node:test";

import { lintFeatures, testFeatures } from "./cargo-features.mjs";
import { environmentForGateStep } from "./gate-env.mjs";
import { plans, stepConfiguration, steps } from "./gate-plan.mjs";

const rustLane = [
  "rustfmt",
  "clippy",
  "clippy-lib",
  "clippy-browser-bridge",
  "rust-fixture",
  "rust-parallel",
  "rust-git-proc",
  "rust-preview",
  "rust-browser-bridge",
  "rust-release",
];

test("Rust gates rebuild the fixture before the existing test phases", () => {
  for (const [mode, plan] of [
    ["full", plans.full],
    ["changed", plans.changed],
  ]) {
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
  assert.ok(!plans.fast.includes("rust-fixture"));
});

test("the local tiers follow the pull-request and after-merge split", () => {
  assert.deepEqual(Object.keys(plans), ["fast", "changed", "full"]);
  // fast is pull-request CI less the Rust tests; changed is all of it; full
  // adds this host's after-merge lanes and the visual suite.
  assert.deepEqual(plans.fast, [
    "policy",
    "prettier",
    "eslint",
    "vitest",
    "budgets",
    "build",
    "web-bundle",
    ...rustLane.slice(0, 4),
  ]);
  assert.deepEqual(plans.changed, [...plans.fast, ...rustLane.slice(4)]);
  assert.deepEqual(plans.full, [...plans.changed, "ui"]);
  // The policy step is CI's policy command, and the release check mirrors
  // CI's bridge-and-release job.
  assert.deepEqual(stepConfiguration("policy").command, [
    "npm",
    "run",
    "check:policy",
  ]);
  assert.deepEqual(stepConfiguration("rust-release").command, [
    "cargo",
    "check",
    "-p",
    "sage",
    "--release",
    "--lib",
  ]);
});

test("fixture build shares the Rust tests' features and target environment", () => {
  const fixture = stepConfiguration("rust-fixture");
  assert.deepEqual(fixture.command, [
    "cargo",
    "build",
    "--workspace",
    "--features",
    testFeatures().join(","),
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

test("Clippy lints every feature that builds on this platform", () => {
  const clippy = stepConfiguration("clippy").command;
  assert.ok(!clippy.includes("--all-features"));
  assert.equal(
    clippy[clippy.indexOf("--features") + 1],
    lintFeatures().join(","),
  );
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
