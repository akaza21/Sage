import { lintFeatures, testFeatures } from "./cargo-features.mjs";
import { environmentForGateStep } from "./gate-env.mjs";

const manifest = "src-tauri/Cargo.toml";
const cargoConfiguration = [
  "--manifest-path",
  manifest,
  "--features",
  testFeatures().join(","),
];
const cargoTest = ["cargo", "test", ...cargoConfiguration];
// Playwright specs share the tests/ui tree with Vitest files but run under
// Playwright only.
const vitestExclude = ["--exclude", "tests/ui/**/*.spec.ts"];

export const steps = {
  policy: ["node", "scripts/generate-acp-fixture-cases.mjs", "--check"],
  "policy-modules": ["node", "scripts/check-policy.mjs"],
  prettier: ["npx", "prettier", "--check", "."],
  eslint: ["npx", "eslint", "."],
  typescript: ["npx", "tsc", "--noEmit"],
  rustfmt: [
    "cargo",
    "fmt",
    "--manifest-path",
    manifest,
    "--all",
    "--",
    "--check",
  ],
  vitest: {
    command: ["npx", "vitest", "run", ...vitestExclude],
    env: environmentForGateStep("vitest"),
  },
  clippy: [
    "cargo",
    "clippy",
    "--manifest-path",
    manifest,
    "--all-targets",
    "--features",
    lintFeatures().join(","),
    "--",
    "-D",
    "warnings",
  ],
  "rust-fixture": [
    "cargo",
    "build",
    ...cargoConfiguration,
    "--bin",
    "acp-adversarial-fixture",
  ],
  "rust-parallel": [
    ...cargoTest,
    "--",
    "--skip",
    "repository_mutation_commands::tests::",
    "--skip",
    "app_preview",
  ],
  "rust-git-proc": [
    ...cargoTest,
    "repository_mutation_commands::tests::",
    "--",
    "--test-threads=1",
  ],
  "rust-preview": [...cargoTest, "app_preview", "--", "--test-threads=1"],
  budgets: ["node", "scripts/budget-lanes.mjs", "--check"],
  build: ["npm", "run", "build"],
  "web-bundle": ["node", "scripts/verify-web-bundle.mjs"],
  ui: ["npm", "run", "test:ui"],
};

export const plans = {
  static: [
    "policy",
    "policy-modules",
    "prettier",
    "eslint",
    "typescript",
    "rustfmt",
  ],
  // fast is the standing landing gate: it deliberately excludes the Rust test
  // phases, budgets, build, web-bundle, and visual suite; full is the pre-merge
  // coverage gate. `changed` (see changedPlan) checks only what a diff needs.
  fast: [
    "policy",
    "policy-modules",
    "prettier",
    "eslint",
    "typescript",
    "rustfmt",
    "vitest",
    "clippy",
  ],
  rust: ["rust-fixture", "rust-parallel", "rust-git-proc", "rust-preview"],
  full: [
    "policy",
    "policy-modules",
    "prettier",
    "eslint",
    "typescript",
    "rustfmt",
    "vitest",
    "clippy",
    "rust-fixture",
    "rust-parallel",
    "rust-git-proc",
    "rust-preview",
    "budgets",
    "build",
    "web-bundle",
    "ui",
  ],
};

export function stepConfiguration(name) {
  const configured = steps[name];
  return Array.isArray(configured)
    ? { command: configured, env: process.env }
    : configured;
}

const staticSteps = ["policy", "policy-modules"];
const rustSteps = ["rustfmt", "clippy", ...plans.rust];

// Steps that take file names start each tool's Node entry point directly, so
// no shell reinterprets a name and Windows' 32,767-character command line
// limit is the only bound; a longer list checks everything instead.
const fileArgumentBudget = 24_000;
const toolEntry = {
  prettier: "node_modules/prettier/bin/prettier.cjs",
  eslint: "node_modules/eslint/bin/eslint.js",
  vitest: "node_modules/vitest/vitest.mjs",
};

// The steps a change needs while it is being written: formatting and lint of
// the changed files, the type check, the Vitest files that import them, the
// Node budget tests when something they read changed, and the Rust lane when
// CI would run it. A step covers the whole tree when the change can affect
// files it does not name. `full` stays the gate before merge.
export function changedPlan(scope) {
  const entries = staticSteps.map(namedStep);
  const scoped = (name, files, args) => {
    if (files === "all" || fitsCommandLine(files) === false) {
      entries.push(namedStep(name));
    } else if (files.length > 0) {
      entries.push({
        name,
        command: ["node", toolEntry[name], ...args, ...files],
        env: stepConfiguration(name).env,
      });
    }
  };
  scoped("prettier", scope.format, ["--check", "--ignore-unknown"]);
  scoped("eslint", scope.lint, ["--no-warn-ignored"]);
  if (scope.typescript || scope.vitest === "all") {
    entries.push(namedStep("typescript"));
  }
  scoped("vitest", scope.vitest, [
    "related",
    "--run",
    "--passWithNoTests",
    ...vitestExclude,
  ]);
  if (scope.nodeTests) entries.push(namedStep("budgets"));
  if (scope.rust) entries.push(...rustSteps.map(namedStep));
  return entries;
}

function fitsCommandLine(files) {
  return (
    files.reduce((length, file) => length + file.length + 3, 0) <=
    fileArgumentBudget
  );
}

function namedStep(name) {
  return { name, ...stepConfiguration(name) };
}
