import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import {
  declaredCargoFeatures,
  lintFeatures,
  platformFeatures,
  testFeatures,
} from "./cargo-features.mjs";

const workflowPath = path.join(process.cwd(), ".github/workflows/ci.yml");

function jobBlock(workflow, jobId) {
  const marker = `  ${jobId}:\n`;
  const start = workflow.indexOf(marker);
  assert.notEqual(start, -1, `CI must define the ${jobId} job`);
  const remainder = workflow.slice(start + marker.length);
  const nextJob = remainder.search(/^ {2}[a-z][a-z0-9-]*:\n/m);
  const end =
    nextJob === -1 ? workflow.length : start + marker.length + nextJob;
  return workflow.slice(start, end);
}

const formatCommand =
  "cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check";
const clippyCommand =
  'cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --features "${{ matrix.lint-features }}" -- -D warnings';
const rustTestPhases = [
  'cargo test --manifest-path src-tauri/Cargo.toml --features "${{ matrix.test-features }}" -- --skip repository_mutation_commands::tests:: --skip app_preview',
  'cargo test --manifest-path src-tauri/Cargo.toml --features "${{ matrix.test-features }}" repository_mutation_commands::tests:: -- --test-threads=1',
  'cargo test --manifest-path src-tauri/Cargo.toml --features "${{ matrix.test-features }}" app_preview -- --test-threads=1',
];
const runnerPlatforms = {
  "windows-latest": "win32",
  "ubuntu-latest": "linux",
  "macos-latest": "darwin",
};
const releaseCheckCommand =
  "cargo check --manifest-path src-tauri/Cargo.toml --release --lib";

test("Rust CI runs the supported feature phases in strict job order", async () => {
  const workflow = await readFile(workflowPath, "utf8");
  const rust = jobBlock(workflow, "rust");
  const cargoCommands = rust
    .split("\n")
    .map((line) => line.trim().replace(/^-\s*/, ""))
    .filter((line) => /^run: ["']?cargo /u.test(line))
    .map((line) => line.slice("run: ".length).replace(/^(["'])(.*)\1$/u, "$2"));

  assert.deepEqual(cargoCommands, [
    formatCommand,
    clippyCommand,
    ...rustTestPhases,
    releaseCheckCommand,
  ]);
  assert.deepEqual(
    cargoCommands.filter((command) => command.startsWith("cargo test ")),
    rustTestPhases,
  );
});

function matrixEntries(rust) {
  const entries = [
    ...rust.matchAll(
      /^ {10}- os: (\S+)\n {12}lint-features: (\S+)\n {12}test-features: (\S+)$/gm,
    ),
  ].map(([, os, lint, tests]) => ({ os, lint, tests }));
  assert.equal(
    [...rust.matchAll(/^ {10}- os: /gm)].length,
    entries.length,
    "every Rust matrix entry must name its lint and test features",
  );
  return entries;
}

test("Rust CI names each runner's features from cargo-features.mjs", async () => {
  const workflow = await readFile(workflowPath, "utf8");
  const entries = matrixEntries(jobBlock(workflow, "rust"));

  assert.deepEqual(
    entries.map(({ os }) => os),
    Object.keys(runnerPlatforms),
  );
  for (const { os, lint, tests } of entries) {
    const platform = runnerPlatforms[os];
    assert.equal(lint, lintFeatures(platform).join(","), `${os} lint features`);
    assert.equal(
      tests,
      testFeatures(platform).join(","),
      `${os} test features`,
    );
  }
});

test("Clippy covers every declared Cargo feature on some runner", () => {
  const declared = declaredCargoFeatures();
  for (const feature of Object.keys(platformFeatures)) {
    assert.ok(declared.includes(feature), `${feature} is a declared feature`);
  }
  const linted = new Set(
    Object.values(runnerPlatforms).flatMap((platform) =>
      lintFeatures(platform),
    ),
  );
  assert.deepEqual([...linted].sort(), declared);
});

test("Cargo feature parsing reads only the [features] table", () => {
  const manifest = [
    "[package]",
    'name = "x"',
    "",
    "[features]",
    "default = []",
    "# a comment",
    'b-lane = ["dep:b"]',
    "a = []",
    "",
    "[profile.dev]",
    "incremental = true",
  ].join("\r\n");
  assert.deepEqual(declaredCargoFeatures(manifest), ["a", "b-lane"]);
  assert.deepEqual(lintFeatures("win32", ["cdp-lane", "x"]), ["x"]);
  assert.deepEqual(testFeatures("linux", ["cdp-lane", "x"]), ["x"]);
});
