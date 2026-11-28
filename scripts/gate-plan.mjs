import { lintFeatures, testFeatures } from "./cargo-features.mjs";
import { environmentForGateStep } from "./gate-env.mjs";

const cargoConfiguration = [
  "--workspace",
  "--features",
  testFeatures().join(","),
];
const cargoTest = ["cargo", "test", ...cargoConfiguration];
// Playwright specs share the tests/ui tree with Vitest files but run under
// Playwright only.
const vitestExclude = ["--exclude", "tests/ui/**/*.spec.ts"];

export const steps = {
  policy: ["npm", "run", "check:policy"],
  prettier: ["npx", "prettier", "--check", "."],
  eslint: ["npx", "eslint", "."],
  rustfmt: ["cargo", "fmt", "--all", "--", "--check"],
  vitest: {
    command: ["npx", "vitest", "run", ...vitestExclude],
    env: environmentForGateStep("vitest"),
  },
  clippy: [
    "cargo",
    "clippy",
    "--workspace",
    "--all-targets",
    "--features",
    lintFeatures().join(","),
    "--",
    "-D",
    "warnings",
  ],
  // The library targets alone, so no dev-dependency turns on a crate's
  // `test-support` feature and the production configuration is linted too.
  "clippy-lib": [
    "cargo",
    "clippy",
    "--workspace",
    "--lib",
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
  "clippy-browser-bridge": [
    "cargo",
    "clippy",
    "-p",
    "sage",
    "--all-targets",
    "--features",
    "browser-bridge",
    "--",
    "-D",
    "warnings",
  ],
  "rust-browser-bridge": [
    "cargo",
    "test",
    "-p",
    "sage",
    "--features",
    "browser-bridge",
    "--lib",
    "dev_bridge::",
  ],
  "rust-release": ["cargo", "check", "-p", "sage", "--release", "--lib"],
  // The portable lane, plus this host's native lane: on Windows that is the
  // lane After merge runs on its Windows runner.
  budgets: ["node", "scripts/budget-lanes.mjs", "--check"],
  build: ["npm", "run", "build"],
  "web-bundle": ["node", "scripts/verify-web-bundle.mjs"],
  ui: ["npm", "run", "test:ui"],
};

// The local tiers follow CI's split (README, "How changes ship"):
//   fast     pull-request CI less the Rust test phases
//   changed  everything pull-request CI runs (every pull request runs all
//            of it: no path rule can safely skip a lane)
//   full     changed, plus what After merge runs on this host (the Rust
//            lanes on this OS, the native budget lane) and the visual
//            suite, which no CI job runs
// The build step runs tsc, so no tier needs a separate typescript step.
const pullRequestLanes = [
  "policy",
  "prettier",
  "eslint",
  "vitest",
  "budgets",
  "build",
  "web-bundle",
];
const rustLintSteps = [
  "rustfmt",
  "clippy",
  "clippy-lib",
  "clippy-browser-bridge",
];
const rustTestSteps = [
  "rust-fixture",
  "rust-parallel",
  "rust-git-proc",
  "rust-preview",
  "rust-browser-bridge",
  "rust-release",
];

const pullRequestCi = [...pullRequestLanes, ...rustLintSteps, ...rustTestSteps];

export const plans = {
  fast: [...pullRequestLanes, ...rustLintSteps],
  changed: pullRequestCi,
  full: [...pullRequestCi, "ui"],
};

export function stepConfiguration(name) {
  const configured = steps[name];
  return Array.isArray(configured)
    ? { command: configured, env: process.env }
    : configured;
}
