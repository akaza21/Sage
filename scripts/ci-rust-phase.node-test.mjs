import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import {
  declaredCargoFeatures,
  lintFeatures,
  platformFeatures,
  runnerFeatures,
  testFeatures,
} from "./cargo-features.mjs";

const workflowPath = path.join(process.cwd(), ".github/workflows/ci.yml");

function runBash(script) {
  const bash =
    process.platform === "win32"
      ? path.join(process.env.ProgramFiles, "Git", "bin", "bash.exe")
      : "bash";
  return spawnSync(bash, ["-s"], {
    input: script,
    encoding: "utf8",
    timeout: 10_000,
    maxBuffer: 1024 * 1024,
  });
}

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

test("Rust CI derives requested runner features from cargo-features.mjs", async () => {
  const workflow = await readFile(workflowPath, "utf8");
  assert.match(
    jobBlock(workflow, "rust"),
    /include: \$\{\{ fromJSON\(needs\.plan\.outputs\.rust-matrix\) \}\}/,
  );
  const plan = jobBlock(workflow, "plan");
  assert.match(
    plan,
    /JSON\.parse\(process\.argv\[1\]\)\.map\(runnerFeatures\)/,
  );
  assert.match(plan, /rust-matrix=\$\(node --input-type=module/);
  for (const [os, platform] of Object.entries(runnerPlatforms)) {
    assert.deepEqual(runnerFeatures(os), {
      os,
      "lint-features": lintFeatures(platform).join(","),
      "test-features": testFeatures(platform).join(","),
    });
  }
  assert.throws(
    () => runnerFeatures("unknown-runner"),
    /unsupported Rust runner/,
  );
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

  // Windows budgets and non-Linux Rust run only on workflow_dispatch.
  let workflowOutsideWindowsLanes = workflow;
  for (const jobId of ["smoke-cli", "playwright"]) {
    const windowsLane = jobBlock(workflow, jobId);
    const output = jobId === "playwright" ? "playwright" : "windows";
    assert.ok(
      windowsLane.includes(`if: needs.plan.outputs.${output} == 'true'`),
    );
    workflowOutsideWindowsLanes = workflowOutsideWindowsLanes.replace(
      windowsLane,
      "",
    );
  }
  for (const line of workflowOutsideWindowsLanes.split("\n")) {
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
    "scripts/cargo-features.mjs",
    ".github/workflows/ci.yml",
  ]) {
    assert.ok(rustPath.test(file), `${file} must run Rust`);
  }
  for (const file of ["README.md", "src/app/App.tsx", "package.json"]) {
    assert.ok(!rustPath.test(file), `${file} alone must not run Rust`);
  }
});

test("every PR plans lanes and gates Playwright, including drafts and ready_for_review", async () => {
  const workflow = await readFile(workflowPath, "utf8");
  const notScheduled = "github.event_name != 'schedule'";
  assert.doesNotMatch(jobBlock(workflow, "security"), /^ {4}if:/m);
  assert.ok(jobBlock(workflow, "plan").includes(`if: ${notScheduled}\n`));
  const mergeGate = jobBlock(workflow, "merge-gate");
  assert.ok(mergeGate.includes(`if: always() && ${notScheduled}\n`));
  assert.match(workflow, /- ready_for_review\n/);
  assert.match(mergeGate, /^ {6}- playwright$/m);
  assert.match(
    mergeGate,
    /PLAYWRIGHT_PLANNED: \$\{\{ needs\.plan\.outputs\.playwright \}\}/,
  );
});

test("PRs and main pushes always plan Playwright independently of optional Windows lanes", async () => {
  const plan = jobBlock(await readFile(workflowPath, "utf8"), "plan");
  const selection = plan.match(/ {10}windows=false\n([\s\S]*?) {10}rust_os=/);
  assert.ok(selection, "the planner must select platform lanes");
  for (const [event, platforms, expected] of [
    ["pull_request", "", "false false true"],
    ["pull_request", "linux", "false false true"],
    ["push", "", "false false true"],
    ["workflow_dispatch", "linux", "false false false"],
    ["workflow_dispatch", "windows", "true false true"],
    ["workflow_dispatch", "macos", "false true false"],
    ["workflow_dispatch", "all", "true true true"],
  ]) {
    const result = runBash(
      `set -euo pipefail\nEVENT=${event}\nPLATFORMS='${platforms}'\nwindows=false\n${selection[1]}\nprintf '%s %s %s' "$windows" "$macos" "$playwright"\n`,
    );
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, expected, `${event}: ${platforms}`);
  }
  assert.match(
    plan,
    /playwright: \$\{\{ steps\.plan\.outputs\.playwright \}\}/,
  );
  assert.match(plan, /echo "playwright=\$playwright"/);
});

test("Merge gate rejects a skipped, cancelled or failed required Playwright lane", async () => {
  const gate = jobBlock(await readFile(workflowPath, "utf8"), "merge-gate");
  const body = gate.split("        run: |\n")[1];
  assert.ok(body, "the Merge gate must enforce lane results");
  for (const [playwright, expectedStatus] of [
    ["success", 0],
    ["skipped", 1],
    ["failure", 1],
    ["cancelled", 1],
  ]) {
    const result = runBash(`
      SECURITY=success
      PLAN=success
      FRONTEND=success
      FRONTEND_PLANNED=true
      SMOKE_CLI=skipped
      SMOKE_CLI_PLANNED=false
      PLAYWRIGHT=${playwright}
      PLAYWRIGHT_PLANNED=true
      RUST=success
      RUST_PLANNED=true
      RUST_OS='["ubuntu-latest"]'
      GITHUB_STEP_SUMMARY=/dev/null
      ${body}
    `);
    assert.equal(result.status, expectedStatus, result.stderr || result.stdout);
    if (expectedStatus !== 0) {
      assert.match(result.stdout, /::error::Windows Playwright:/);
    }
  }
});
