import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import {
  declaredCargoFeatures,
  lintFeatures,
  isolatedDebugFeatures,
  platformFeatures,
  runnerFeatures,
  testFeatures,
} from "./cargo-features.mjs";

const workflowFile = (name) =>
  readFile(path.join(process.cwd(), ".github", name), "utf8");
const ciWorkflow = () => workflowFile("workflows/ci.yml");
const rustWorkflow = () => workflowFile("workflows/rust.yml");
const afterMergeWorkflow = () => workflowFile("workflows/main.yml");
const packagesWorkflow = () => workflowFile("workflows/desktop-packages.yml");

function jobBlock(workflow, jobId) {
  const marker = `  ${jobId}:\n`;
  const start = workflow.indexOf(marker);
  assert.notEqual(start, -1, `the workflow must define the ${jobId} job`);
  const remainder = workflow.slice(start + marker.length);
  const nextJob = remainder.search(/^ {2}[a-z][a-z0-9-]*:\n/m);
  const end =
    nextJob === -1 ? workflow.length : start + marker.length + nextJob;
  return workflow.slice(start, end);
}

function jobIds(workflow) {
  const jobs = workflow.slice(workflow.indexOf("\njobs:\n"));
  return [...jobs.matchAll(/^ {2}([a-z][a-z0-9-]*):\n/gm)].map(
    (match) => match[1],
  );
}

const formatCommand = "cargo fmt --all -- --check";
const clippyCommand =
  'cargo clippy --workspace --all-targets --features "${{ matrix.lint-features }}" -- -D warnings';
const clippyLibCommand =
  'cargo clippy --workspace --lib --features "${{ matrix.lint-features }}" -- -D warnings';
const rustTestPhases = [
  'cargo test --workspace --features "${{ matrix.test-features }}" -- --skip repository_mutation_commands::tests:: --skip app_preview',
  'cargo test --workspace --features "${{ matrix.test-features }}" repository_mutation_commands::tests:: -- --test-threads=1',
  'cargo test --workspace --features "${{ matrix.test-features }}" app_preview -- --test-threads=1',
];
const rustJobs = ["rust-lint", "rust", "rust-bridge"];
const runnerPlatforms = {
  "windows-latest": "win32",
  "ubuntu-latest": "linux",
  "macos-latest": "darwin",
};
const releaseCheckCommand = "cargo check -p sage --release --lib";
const bridgeClippyCommand =
  "cargo clippy -p sage --all-targets --features browser-bridge -- -D warnings";
const bridgeTestCommand =
  "cargo test -p sage --features browser-bridge --lib dev_bridge::";

test("Rust CI runs the supported feature phases in strict job order", async () => {
  const workflow = await rustWorkflow();
  const cargoCommands = (job) =>
    jobBlock(workflow, job)
      .split("\n")
      .map((line) => line.trim().replace(/^-\s*/, ""))
      .filter((line) => /^run: ["']?cargo /u.test(line))
      .map((line) =>
        line.slice("run: ".length).replace(/^(["'])(.*)\1$/u, "$2"),
      );

  // The Rust lane runs as three jobs on separate runners.
  assert.deepEqual(
    Object.fromEntries(rustJobs.map((job) => [job, cargoCommands(job)])),
    {
      "rust-lint": [
        formatCommand,
        clippyCommand,
        clippyLibCommand,
        bridgeClippyCommand,
      ],
      rust: rustTestPhases,
      "rust-bridge": [bridgeTestCommand, releaseCheckCommand],
    },
  );
});

test("Rust CI derives requested runner features from cargo-features.mjs", async () => {
  const workflow = await rustWorkflow();
  for (const job of rustJobs) {
    assert.match(
      jobBlock(workflow, job),
      /include: \$\{\{ fromJSON\(needs\.features\.outputs\.matrix\) \}\}/,
    );
  }
  const features = jobBlock(workflow, "features");
  assert.match(
    features,
    /JSON\.parse\(process\.argv\[1\]\)\.map\(runnerFeatures\)/,
  );
  assert.match(features, /matrix=\$\(node --input-type=module/);
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

test("pull requests run Rust on Linux and after merge on Windows and macOS", async () => {
  const runners = (block) => {
    assert.match(block, /uses: \.\/\.github\/workflows\/rust\.yml/);
    return JSON.parse(block.match(/runners: '(\[[^']*\])'/)[1]);
  };
  assert.deepEqual(runners(jobBlock(await ciWorkflow(), "rust")), [
    "ubuntu-latest",
  ]);
  assert.deepEqual(runners(jobBlock(await afterMergeWorkflow(), "rust")), [
    "windows-latest",
    "macos-latest",
  ]);
});

test("Clippy covers every declared Cargo feature on some runner", async () => {
  const declared = declaredCargoFeatures();
  for (const feature of Object.keys(platformFeatures)) {
    assert.ok(declared.includes(feature), `${feature} is a declared feature`);
  }
  const linted = new Set(
    Object.values(runnerPlatforms).flatMap((platform) =>
      lintFeatures(platform),
    ),
  );
  const workflow = await rustWorkflow();
  for (const feature of isolatedDebugFeatures) {
    assert.ok(
      workflow.includes(
        `run: cargo clippy -p sage --all-targets --features ${feature} -- -D warnings`,
      ),
    );
    linted.add(feature);
  }
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

test("pull request CI stays on Linux with bounded jobs", async () => {
  const workflow = await ciWorkflow();

  assert.match(
    workflow,
    /^concurrency:\n {2}group: .+\n {2}cancel-in-progress: true$/m,
    "a new push cancels the run in flight for the same branch",
  );
  // A push to main has a group of its own run, so no later push (even of the
  // same commit) cancels or replaces it before it saves caches.
  const group = workflow.match(/^ {2}group: (.+)$/m)[1];
  assert.ok(
    group.includes(
      "github.event_name == 'push' && format('push-{0}', github.run_id)",
    ),
    "each main push runs in its own concurrency group",
  );
  assert.match(
    workflow,
    /^ {2}push:\n {4}branches:\n {6}- main$/m,
    "push runs are limited to main",
  );
  // The one requested entry point is the after-merge workflow.
  assert.doesNotMatch(workflow, /workflow_dispatch/);
  assert.doesNotMatch(workflow, /windows-latest|macos-latest/);

  // Every job on a runner has a deadline; a job that calls a workflow takes
  // the called jobs' deadlines.
  for (const file of [
    "ci.yml",
    "rust.yml",
    "main.yml",
    "desktop-packages.yml",
  ]) {
    const text = await workflowFile(`workflows/${file}`);
    for (const jobId of jobIds(text)) {
      const block = jobBlock(text, jobId);
      if (/^ {4}uses: /m.test(block)) continue;
      assert.match(
        block,
        /^ {4}timeout-minutes: \d+$/m,
        `${file} ${jobId} must set timeout-minutes`,
      );
    }
  }
});

test("only Linux runs on main save Rust caches", async () => {
  const setup = await workflowFile("actions/rust-setup/action.yml");
  assert.match(
    setup,
    /save-if: \$\{\{ github\.ref == 'refs\/heads\/main' && runner\.os == 'Linux' \}\}/,
  );
  assert.doesNotMatch(await packagesWorkflow(), /rust-cache/);
});

test("draft pull requests and schedules start only the security job", async () => {
  const workflow = await ciWorkflow();
  const notScheduled = "github.event_name != 'schedule'";
  const notDraft =
    "github.event_name != 'pull_request' || !github.event.pull_request.draft";
  assert.doesNotMatch(jobBlock(workflow, "security"), /^ {4}if:/m);
  for (const job of ["frontend", "vitest", "rust"]) {
    assert.ok(
      jobBlock(workflow, job).includes(`if: ${notScheduled} && (${notDraft})`),
      job,
    );
  }
  const mergeGate = jobBlock(workflow, "merge-gate");
  assert.ok(mergeGate.includes(`always() && ${notScheduled} &&`));
  assert.ok(mergeGate.includes(`(${notDraft})`));
});

test("every pull request runs every lane, and the merge gate requires them all", async () => {
  const workflow = await ciWorkflow();
  const gate = jobBlock(workflow, "merge-gate");
  const needs = gate
    .match(/^ {4}needs:\n((?: {6}- .+\n)+)/m)[1]
    .split("\n")
    .filter(Boolean)
    .map((line) => line.trim().slice(2));
  assert.deepEqual(
    needs,
    jobIds(workflow).filter((id) => id !== "merge-gate"),
  );
  for (const line of [
    'require "Security" "$SECURITY"',
    'require "Frontend and policy" "$FRONTEND"',
    'require "Vitest" "$VITEST"',
    'require "Rust" "$RUST"',
  ]) {
    assert.ok(gate.includes(line), line);
  }
  // No lane is planned away: a lane runs unless the run is a draft or a
  // schedule, and the frontend job runs every step of `npm run check` but
  // Vitest.
  assert.doesNotMatch(workflow, /needs.plan|outputs.(rust|vitest|bundle)/);
  assert.doesNotMatch(jobBlock(workflow, "frontend"), /^ {6}- if:|^ {8}if:/m);
});

test("after merge runs on main and nightly, and no push cancels another", async () => {
  const workflow = await afterMergeWorkflow();
  assert.match(workflow, /^ {2}push:\n {4}branches:\n {6}- main$/m);
  assert.match(workflow, /^ {2}schedule:\n {4}- cron: "[^"]+"$/m);
  assert.match(workflow, /^ {2}workflow_dispatch:$/m);
  assert.match(
    workflow,
    /^ {2}group: after-merge-\$\{\{ github\.event_name == 'workflow_dispatch' && github\.ref \|\| github\.run_id \}\}$/m,
    "pushes and nightly runs each get their own group",
  );

  // Packages build nightly and on request only; every push runs the rest.
  const packages = jobBlock(workflow, "packages");
  assert.match(
    packages,
    /uses: \.\/\.github\/workflows\/desktop-packages\.yml/,
  );
  assert.match(
    packages,
    /^ {4}if: github\.event_name == 'schedule' \|\| github\.event_name == 'workflow_dispatch'$/m,
  );
  for (const job of ["rust", "windows-budget"]) {
    assert.doesNotMatch(jobBlock(workflow, job), /^ {4}if:/m, job);
  }
  assert.match(
    jobBlock(workflow, "windows-budget"),
    /runs-on: windows-latest[\s\S]*run: npm run test:budgets:windows/,
  );
  // Called workflows take their trigger from After merge alone.
  for (const called of [await packagesWorkflow(), await rustWorkflow()]) {
    assert.match(called, /^on:\n {2}workflow_call:/m);
    assert.doesNotMatch(called, /^concurrency:/m);
  }
});

test("after merge reports every other job through one issue each", async () => {
  const workflow = await afterMergeWorkflow();
  const report = jobBlock(workflow, "report");
  const needs = report
    .match(/^ {4}needs:\n((?: {6}- .+\n)+)/m)[1]
    .split("\n")
    .filter(Boolean)
    .map((line) => line.trim().slice(2));
  assert.deepEqual(
    needs,
    jobIds(workflow).filter((id) => id !== "report"),
  );
  assert.match(
    report,
    /if: \$\{\{ !cancelled\(\) && github\.event_name != 'workflow_dispatch' \}\}/,
  );
  assert.match(report, /^ {6}issues: write$/m);
  assert.match(report, /^ {6}actions: read$/m);
  assert.match(report, /REPORTER_NAME: Report failures/);
  assert.match(report, /^ {4}name: Report failures$/m);
  assert.match(report, /run: node scripts\/ci-failure-issues\.mjs/);
  // Reporters never overlap or cancel each other.
  assert.match(
    report,
    /^ {4}concurrency:\n {6}group: after-merge-report\n {6}cancel-in-progress: false$/m,
  );
  // The workflow writes issues only from this job.
  assert.equal(workflow.match(/issues: write/g).length, 1);
});
