import { environmentForGateStep } from "./gate-env.mjs";

const manifest = "src-tauri/Cargo.toml";
const cargoTest = [
  "cargo",
  "test",
  "--manifest-path",
  manifest,
  "--all-features",
];

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
    command: ["npx", "vitest", "run", "--exclude", "tests/ui/**/*.spec.ts"],
    env: environmentForGateStep("vitest"),
  },
  clippy: [
    "cargo",
    "clippy",
    "--manifest-path",
    manifest,
    "--all-targets",
    "--all-features",
    "--",
    "-D",
    "warnings",
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
  // phases, budgets, build, web-bundle, and visual suite; full is the per-wave
  // coverage gate.
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
  rust: ["rust-parallel", "rust-git-proc", "rust-preview"],
  full: [
    "policy",
    "policy-modules",
    "prettier",
    "eslint",
    "typescript",
    "rustfmt",
    "vitest",
    "clippy",
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
