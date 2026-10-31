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

test("CI keeps hosted-runner spend bounded", async () => {
  const workflow = await readFile(workflowPath, "utf8");

  assert.match(
    workflow,
    /^concurrency:\n {2}group: .+\n {2}cancel-in-progress: true$/m,
    "a new push must cancel the run in flight for the same branch",
  );
  assert.match(
    workflow,
    /^ {2}push:\n {4}branches:\n {6}- main$/m,
    "push runs are limited to main",
  );

  const jobIds = [
    ...workflow.matchAll(/^ {2}([a-z][a-z0-9-]*):\n {4}name:/gm),
  ].map((match) => match[1]);
  assert.ok(jobIds.length > 0, "CI must define jobs");
  for (const jobId of jobIds) {
    assert.match(
      jobBlock(workflow, jobId),
      /^ {4}timeout-minutes: \d+$/m,
      `${jobId} must set timeout-minutes`,
    );
  }

  // Windows and macOS run only when requested through workflow_dispatch.
  const smokeCli = jobBlock(workflow, "smoke-cli");
  assert.match(smokeCli, /if: needs\.plan\.outputs\.windows == 'true'/);
  const workflowOutsideSmokeCli = workflow.replace(smokeCli, "");
  for (const line of workflowOutsideSmokeCli.split("\n")) {
    if (/windows-latest|macos-latest/.test(line)) {
      assert.match(
        line,
        /rust_os\+=/,
        `a Windows or macOS runner may only be added on request: ${line.trim()}`,
      );
    }
  }
  assert.match(jobBlock(workflow, "plan"), /case "\$\{PLATFORMS:-linux\}" in/);
});

test("CI plans Rust from both sides of every changed path", async () => {
  const plan = jobBlock(await readFile(workflowPath, "utf8"), "plan");
  const rustPaths = plan.match(/^ {10}RUST_PATHS: (.+)$/m);
  assert.ok(rustPaths, "the plan job must name the Rust paths");
  const rustPath = new RegExp(rustPaths[1]);

  // Each changed-file listing emits a rename's old path as well as its new one.
  assert.match(
    plan,
    /^ {10}names='\.filename, \(\.previous_filename \/\/ empty\)'$/m,
  );
  assert.match(
    plan,
    /pulls\/\$PR_NUMBER\/files" \\\n\s*--jq "\.\[\] \| \$names"/,
  );
  assert.match(
    plan,
    /compare\/\$BEFORE\.\.\.\$AFTER" \\\n\s*--jq "\.files\[\] \| \$names"/,
  );

  const renamedOutOfRust = {
    filename: "src/app/moved.ts",
    previous_filename: "src-tauri/src/moved.rs",
  };
  const listed = [
    renamedOutOfRust.filename,
    renamedOutOfRust.previous_filename,
  ];
  assert.ok(listed.some((file) => rustPath.test(file)));

  for (const file of [
    "src-tauri/src/lib.rs",
    "testdata/acp/codex/config-v1.toml",
    "tests/fixtures/harness-traces/claude-code/smoke/frames.jsonl",
    "src/features/chat/acp/rendererRecreationPacket.ts",
    "rust-toolchain.toml",
    ".github/workflows/ci.yml",
  ]) {
    assert.ok(rustPath.test(file), `${file} must run Rust`);
  }
  for (const file of ["README.md", "src/app/App.tsx", "package.json"]) {
    assert.ok(!rustPath.test(file), `${file} alone must not run Rust`);
  }
});

test("draft pull requests start no jobs", async () => {
  const workflow = await readFile(workflowPath, "utf8");
  const notDraft =
    "github.event_name != 'pull_request' || !github.event.pull_request.draft";
  assert.ok(jobBlock(workflow, "plan").includes(`if: ${notDraft}`));
  const mergeGate = jobBlock(workflow, "merge-gate");
  assert.ok(mergeGate.includes("always() &&"));
  assert.ok(mergeGate.includes(`(${notDraft})`));
});
