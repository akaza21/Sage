import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

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
  "cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings";
const rustTestPhases = [
  "cargo test --manifest-path src-tauri/Cargo.toml --all-features -- --skip repository_mutation_commands::tests:: --skip app_preview",
  "cargo test --manifest-path src-tauri/Cargo.toml --all-features repository_mutation_commands::tests:: -- --test-threads=1",
  "cargo test --manifest-path src-tauri/Cargo.toml --all-features app_preview -- --test-threads=1",
];
const releaseCheckCommand =
  "cargo check --manifest-path src-tauri/Cargo.toml --release --lib";

test("Rust CI runs the supported all-feature phases in strict job order", async () => {
  const workflow = await readFile(workflowPath, "utf8");
  const rust = jobBlock(workflow, "rust");
  const cargoCommands = rust
    .split("\n")
    .map((line) => line.trim().replace(/^-\s*/, ""))
    .filter(
      (line) =>
        line.startsWith("run: cargo ") || line.startsWith('run: "cargo '),
    )
    .map((line) => line.slice("run: ".length).replace(/^"|"$/g, ""));

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
