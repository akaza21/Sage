import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const workflowPath = path.join(process.cwd(), ".github/workflows/ci.yml");

function jobBlock(workflow, jobId) {
  const job = workflow.match(
    new RegExp(
      `^  ${jobId}:\\n([\\s\\S]*?)(?=^  [a-z][a-z0-9-]*:\\n|(?![\\s\\S]))`,
      "m",
    ),
  );
  assert.ok(job, `CI must define the ${jobId} job`);
  return job[0];
}

test("BETA-0001 macOS CI compiles and tests the named shared contracts", async () => {
  const workflow = await readFile(workflowPath, "utf8");
  const plan = jobBlock(workflow, "plan");
  const rust = jobBlock(workflow, "rust");
  const mergeGate = jobBlock(workflow, "merge-gate");

  // The macOS leg is part of the full Rust matrix, requested on demand.
  assert.match(
    rust,
    /include:\s*\$\{\{\s*fromJSON\(needs\.plan\.outputs\.rust-matrix\)\s*\}\}/,
  );
  assert.match(plan, /macos\) macos=true ;;/);
  assert.match(plan, /all\) windows=true; macos=true ;;/);
  assert.match(plan, /rust_os\+=',"macos-latest"'/);

  const placeholder =
    "node -e \"require('node:fs').mkdirSync('dist', { recursive: true })\"";
  assert.ok(
    rust.includes(placeholder),
    "Rust matrix must create the Tauri frontend placeholder",
  );
  assert.ok(
    rust.indexOf(placeholder) <
      rust.indexOf(
        'cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --features "${{ matrix.lint-features }}" -- -D warnings',
      ),
    "Rust frontend placeholder must be created before Clippy",
  );
  for (const command of [
    "cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check",
    'cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --features "${{ matrix.lint-features }}" -- -D warnings',
    "cargo check --manifest-path src-tauri/Cargo.toml --release --lib",
  ]) {
    assert.ok(
      rust.includes(command),
      `full Rust matrix must retain ${command}`,
    );
  }

  // The parallel test phase runs the shared-contract suites on every leg.
  const parallelPhase = rust.match(
    /name:\s*Test Rust parallel phase\s*\n\s*run:\s*'([^']+)'/,
  );
  assert.ok(parallelPhase, "Rust matrix must run the parallel test phase");
  for (const suite of ["workspace::notify", "editor_watch"]) {
    assert.ok(
      !parallelPhase[1].includes(`--skip ${suite}`),
      `the parallel phase must not skip ${suite}`,
    );
  }

  assert.match(mergeGate, /needs:[\s\S]*?-\s*plan[\s\S]*?-\s*rust/);
  assert.match(mergeGate, /require "Rust" "\$RUST_PLANNED" "\$RUST"/);
});
