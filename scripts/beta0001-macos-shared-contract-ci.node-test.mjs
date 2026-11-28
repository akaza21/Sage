import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const workflowPath = path.join(process.cwd(), ".github/workflows/ci.yml");
const setupActionPath = path.join(
  process.cwd(),
  ".github/actions/rust-setup/action.yml",
);

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
  const rustJobs = ["rust-lint", "rust", "rust-bridge"].map((job) =>
    jobBlock(workflow, job),
  );
  const rust = rustJobs[1];
  const mergeGate = jobBlock(workflow, "merge-gate");

  // The macOS leg is part of the full Rust matrix, requested on demand.
  for (const job of rustJobs) {
    assert.match(
      job,
      /include:\s*\$\{\{\s*fromJSON\(needs\.plan\.outputs\.rust-matrix\)\s*\}\}/,
    );
  }
  assert.match(plan, /macos\) macos=true ;;/);
  assert.match(plan, /all\) windows=true; macos=true ;;/);
  assert.match(plan, /rust_os\+=',"macos-latest"'/);

  const placeholder =
    "node -e \"require('node:fs').mkdirSync('dist', { recursive: true })\"";
  const setup = "uses: ./.github/actions/rust-setup";
  assert.ok(
    (await readFile(setupActionPath, "utf8")).includes(placeholder),
    "the Rust setup action must create the Tauri frontend placeholder",
  );
  // Every Rust job builds `sage`, whose generate_context! needs dist/.
  for (const job of rustJobs) {
    const firstBuild = job.search(/run: ["']?cargo (?:clippy|test|check) /);
    assert.ok(firstBuild > 0, "Rust job must build sage");
    assert.ok(
      job.includes(setup) && job.indexOf(setup) < firstBuild,
      "Rust setup, with its frontend placeholder, must run before the first build",
    );
  }
  for (const command of [
    "cargo fmt --all -- --check",
    'cargo clippy --workspace --all-targets --features "${{ matrix.lint-features }}" -- -D warnings',
    "cargo check -p sage --release --lib",
  ]) {
    assert.ok(
      rustJobs.some((job) => job.includes(command)),
      `full Rust matrix must retain ${command}`,
    );
  }

  // The parallel test phase runs the shared-contract suites on every leg.
  const parallelPhase = rust.match(
    /name:\s*Test Rust parallel phase\s*\n\s*run:\s*'([^']+)'/,
  );
  assert.ok(parallelPhase, "Rust matrix must run the parallel test phase");
  for (const suite of ["workspace_notify", "editor_watch"]) {
    assert.ok(
      !parallelPhase[1].includes(`--skip ${suite}`),
      `the parallel phase must not skip ${suite}`,
    );
  }

  assert.match(mergeGate, /needs:[\s\S]*?-\s*plan[\s\S]*?-\s*rust/);
  assert.match(mergeGate, /require "Rust lint" "\$RUST_PLANNED" "\$RUST_LINT"/);
  assert.match(mergeGate, /require "Rust" "\$RUST_PLANNED" "\$RUST"/);
  assert.match(
    mergeGate,
    /require "Rust bridge and release" "\$RUST_PLANNED" "\$RUST_BRIDGE"/,
  );
});
