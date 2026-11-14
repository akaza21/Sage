import assert from "node:assert/strict";
import test from "node:test";

import { lintFeatures, testFeatures } from "./cargo-features.mjs";
import { environmentForGateStep } from "./gate-env.mjs";
import { changeScope } from "./change-scope.mjs";
import { changedPlan, plans, stepConfiguration, steps } from "./gate-plan.mjs";

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

const stepNames = (files, exists) =>
  changedPlan(changeScope(files, exists)).map(({ name }) => name);
const step = (files, name, exists) =>
  changedPlan(changeScope(files, exists)).find((entry) => entry.name === name);

test("changed gate checks a UI change with only the tests that import it", () => {
  const files = ["src/app/App.tsx", "src/app/app.css", "README.md"];
  assert.deepEqual(stepNames(files), [
    "policy",
    "policy-modules",
    "prettier",
    "eslint",
    "typescript",
    "vitest",
  ]);
  assert.deepEqual(step(files, "prettier").command.slice(-3), files);
  assert.deepEqual(step(files, "eslint").command.slice(-1), [
    "src/app/App.tsx",
  ]);
  const vitest = step(files, "vitest").command;
  assert.ok(vitest.includes("related"));
  assert.deepEqual(vitest.slice(-2), ["src/app/App.tsx", "src/app/app.css"]);
  // Playwright specs import src modules too, but only Playwright runs them.
  assert.equal(
    vitest[vitest.indexOf("--exclude") + 1],
    "tests/ui/**/*.spec.ts",
  );
});

test("changed gate traces Vitest from Playwright fixtures", () => {
  const vitest = step(
    ["tests/ui/fixtures/agentTurnJourneyFixture.ts"],
    "vitest",
  );
  assert.deepEqual(vitest.command.slice(-1), [
    "tests/ui/fixtures/agentTurnJourneyFixture.ts",
  ]);
});

test("changed gate runs the Rust lane exactly when CI would", () => {
  const rustLane = ["rustfmt", "clippy", ...plans.rust];
  for (const files of [
    ["src-tauri/src/lib.rs"],
    ["src/features/chat/acp/rendererRecreationPacket.ts"],
    // A rename out of the crate still runs Rust from its old path.
    ["src/app/moved.ts", "src-tauri/src/moved.rs"],
  ]) {
    const exists = (file) => !file.startsWith("src-tauri/src/moved");
    assert.deepEqual(
      stepNames(files, exists).slice(-rustLane.length),
      rustLane,
      files[0],
    );
  }
  assert.ok(!stepNames(["src/app/App.tsx"]).includes("clippy"));
});

test("changed gate checks the whole tree when shared configuration changes", () => {
  for (const [file, names] of [
    ["package.json", ["prettier", "eslint", "vitest"]],
    ["vite.config.ts", ["vitest"]],
    ["src/test/setup.ts", ["vitest"]],
    ["eslint.config.js", ["eslint"]],
    [".prettierrc.json", ["prettier"]],
  ]) {
    for (const name of names) {
      assert.deepEqual(
        step([file], name),
        {
          name,
          ...stepConfiguration(name),
        },
        `${file} must run all of ${name}`,
      );
    }
  }
});

test("changed gate type-checks a deletion but formats only present files", () => {
  const exists = (file) => file.endsWith(".md");
  const files = ["src/app/gone.ts", "docs/notes.md"];
  assert.deepEqual(stepNames(files, exists), [
    "policy",
    "policy-modules",
    "prettier",
    "typescript",
  ]);
  assert.deepEqual(step(files, "prettier", exists).command.slice(-1), [
    "docs/notes.md",
  ]);
});

test("changed gate runs the Node budget tests when something they read changes", () => {
  for (const file of [
    "scripts/lane.mjs",
    ".github/workflows/ci.yml",
    "src-tauri/Cargo.toml",
  ]) {
    assert.ok(stepNames([file]).includes("budgets"), file);
  }
  assert.ok(!stepNames(["src/app/App.tsx"]).includes("budgets"));
});

test("changed gate checks everything rather than overflow a command line", () => {
  const files = Array.from({ length: 2_000 }, (_, i) => `src/app/m${i}.ts`);
  for (const name of ["prettier", "eslint", "vitest"]) {
    assert.deepEqual(step(files, name), { name, ...stepConfiguration(name) });
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
