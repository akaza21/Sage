import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { Readable, Writable } from "node:stream";
import test from "node:test";
import { pathToFileURL } from "node:url";

import {
  acquireCargoTargetLease,
  cargoLeasePath,
  canonicalCargoTarget,
} from "./lib/check-cargo-lease.mjs";
import {
  buildRustEnvironment,
  CheckProcessError,
  executeCommand,
  runCommandGroups,
} from "./lib/check-process.mjs";
import { consumeWindowsCargoJobFrames } from "./lib/check-windows-cargo-protocol.mjs";
import {
  createRustValidationCommands,
  resolveNpmInvocation,
  VALIDATION_DEADLINES_MS,
} from "./lib/check-plans.mjs";
import { createDevPlan, parseDevArguments } from "./check-dev.mjs";
import { createIntegrationPlan } from "./check-integration.mjs";
import { createReleasePlan, parseReleaseArguments } from "./check-release.mjs";

const repositoryRoot = "C:\\workspace\\sage";
// origin: check-lanes.node-test.mjs:34 "Windows Cargo frame parser rejects reordered and post-proof frames" -> check-lanes.portable.node-test.mjs
test("Windows Cargo frame parser rejects reordered and post-proof frames", async () => {
  const requestNonce = "test-nonce";
  const proof = {
    type: "proof",
    schemaVersion: 1,
    requestNonce,
    sequence: 0,
    settled: true,
    treeBoundaryVerified: true,
    activeProcesses: 0,
    exitCode: 0,
    reason: null,
    teardown: null,
  };
  const data = {
    type: "data",
    schemaVersion: 1,
    requestNonce,
    sequence: 1,
    stream: "stdout",
    data: Buffer.from("unexpected").toString("base64"),
  };
  for (const frames of [[data], [proof, data]]) {
    await assert.rejects(
      consumeWindowsCargoJobFrames(
        Readable.from(
          frames.map((frame) => Buffer.from(`${JSON.stringify(frame)}\n`)),
        ),
        {
          label: "protocol",
          maximumLineBytes: 32,
          maximumOutputBytes: 32,
        },
        requestNonce,
        discardStream(),
        discardStream(),
      ),
      /invalid Job protocol frame|frame followed terminal proof/u,
    );
  }
});

// origin: Q01_VCTIP_TEARDOWN "portable protocol receipt validation"
test("Windows Cargo frame parser accepts only the exact VCTIP teardown receipt", async () => {
  const requestNonce = "vctip-receipt-nonce";
  const base = {
    type: "proof",
    schemaVersion: 1,
    requestNonce,
    sequence: 0,
    settled: true,
    treeBoundaryVerified: true,
    activeProcesses: 0,
    exitCode: 7,
    reason: null,
  };
  const receipt = {
    kind: "verified-msvc-vctip",
    imagePath:
      "C:\\Program Files (x86)\\Microsoft Visual Studio\\2022\\BuildTools\\VC\\Tools\\MSVC\\14.44.35207\\bin\\HostX64\\x64\\VCTIP.EXE",
    originalFilename: "VCTIP.EXE",
    imageSizeBytes: 335328,
    sha256: "e175c981ca866689bf8f8d9bd5ba1e71e2eb1c9d389de555744b0cb439250996",
    fileVersion: "14.44.35228.0",
    productVersion: "14.44.35228.0",
    singletonPid: 29_084,
    rootExited: true,
    jobMembershipVerified: true,
    terminationExitCodeRequested: 0x53475654,
    terminationExitCodeObserved: 0x53475654,
  };
  const unconfirmed = {
    ...receipt,
    kind: "msvc-vctip-termination-unconfirmed",
    terminationObservation: "wait-timeout",
  };
  delete unconfirmed.terminationExitCodeObserved;
  for (const teardown of [null, receipt, unconfirmed]) {
    const proof = await consumeWindowsCargoJobFrames(
      Readable.from([
        Buffer.from(`${JSON.stringify({ ...base, teardown })}\n`),
      ]),
      { label: "protocol", maximumLineBytes: 32, maximumOutputBytes: 32 },
      requestNonce,
      discardStream(),
      discardStream(),
    );
    assert.deepEqual(proof.teardown, teardown);
  }
  for (const teardown of [
    { ...receipt, kind: "other" },
    { ...receipt, sha256: receipt.sha256.toUpperCase() },
    { ...receipt, fileVersion: "14.44.35229.0" },
    { ...receipt, unexpected: true },
    Object.fromEntries(
      Object.entries(receipt).filter(([key]) => key !== "productVersion"),
    ),
    { ...receipt, singletonPid: 0 },
    { ...unconfirmed, terminationObservation: "other" },
    { ...unconfirmed, terminationExitCodeObserved: 0x53475654 },
  ]) {
    await assert.rejects(
      consumeWindowsCargoJobFrames(
        Readable.from([
          Buffer.from(`${JSON.stringify({ ...base, teardown })}\n`),
        ]),
        { label: "protocol", maximumLineBytes: 32, maximumOutputBytes: 32 },
        requestNonce,
        discardStream(),
        discardStream(),
      ),
      /invalid VCTIP teardown receipt/u,
    );
  }
});

// origin: check-lanes.node-test.mjs:75 "developer lane requires explicit bounded targets" -> check-lanes.portable.node-test.mjs
test("developer lane requires explicit bounded targets", () => {
  assert.throws(() => parseDevArguments([]), /at least one/);
  assert.throws(
    () => parseDevArguments(["--rust-filter", ""]),
    /bounded value/,
  );
  assert.throws(
    () => parseDevArguments(["--vitest", "--runInBand"]),
    /bounded value/,
  );
  assert.throws(
    () => parseDevArguments(["--rust-filter", "*"]),
    /bounded value/,
  );
  assert.throws(() => parseDevArguments(["--vitest", "."]), /bounded value/);
  assert.throws(() => parseDevArguments(["--vitest", "src"]), /bounded value/);
  for (const filter of ["tests", "::", "module::", "::test"]) {
    assert.throws(
      () => parseDevArguments(["--rust-filter", filter]),
      /concrete module::test/u,
    );
  }
  for (const target of ["test", "src/chat.tsx", "src/test"]) {
    assert.throws(
      () => parseDevArguments(["--vitest", target]),
      /concrete test\/spec file/u,
    );
  }
  assert.throws(
    () =>
      parseDevArguments(
        Array.from({ length: 17 }, (_, index) => [
          "--rust-filter",
          `module::test_${index}`,
        ]).flat(),
      ),
    /at most 16/,
  );
});

// origin: check-lanes.node-test.mjs:115 "developer plan keeps Cargo filters sequential and overlaps only language lanes" -> check-lanes.portable.node-test.mjs
test("developer plan keeps Cargo filters sequential and overlaps only language lanes", () => {
  const options = parseDevArguments([
    "--rust-filter",
    "module::first_test",
    "--vitest",
    "src/workbench.test.tsx",
    "--rust-filter",
    "module::second_test",
    "--dry-run",
  ]);
  const plan = createDevPlan(options, {
    platform: "linux",
    repositoryRoot,
    rustEnvironment: {},
  });

  assert.equal(plan.groups.length, 2);
  assert.equal(plan.groups[0].concurrency, 1);
  assert.equal(plan.groups[0].timeoutMs, VALIDATION_DEADLINES_MS.devLane);
  assert.equal(plan.groups[1].timeoutMs, VALIDATION_DEADLINES_MS.devLane);
  assert.ok(
    plan.groups
      .flatMap(({ commands }) => commands)
      .every(({ timeoutMs }) => Number.isInteger(timeoutMs)),
  );
  assert.deepEqual(
    plan.groups[0].commands.map(({ command, args }) => [command, args]),
    [
      [
        "cargo",
        [
          "test",
          "--manifest-path",
          "src-tauri/Cargo.toml",
          "--all-features",
          "module::first_test",
        ],
      ],
      [
        "cargo",
        [
          "test",
          "--manifest-path",
          "src-tauri/Cargo.toml",
          "--all-features",
          "module::second_test",
        ],
      ],
    ],
  );
  assert.deepEqual(plan.groups[1].commands[0].args, [
    "run",
    "test",
    "--",
    "src/workbench.test.tsx",
  ]);
});

// origin: check-lanes.node-test.mjs:173 "Rust validation plans build the frontend bundle before concurrent groups" -> check-lanes.portable.node-test.mjs
test("Rust validation plans build the frontend bundle before concurrent groups", () => {
  const npmInvocation = {
    command: "C:\\node\\node.exe",
    argsPrefix: ["C:\\node\\node_modules\\npm\\bin\\npm-cli.js"],
  };
  const developer = createDevPlan(
    parseDevArguments(["--rust-filter", "module::focused_test"]),
    {
      platform: "win32",
      repositoryRoot,
      rustEnvironment: { CARGO_TARGET_DIR: "C:\\SageCaches\\target" },
      npmInvocation,
    },
  );
  const integration = createIntegrationPlan({
    platform: "win32",
    repositoryRoot,
    rustEnvironment: { CARGO_TARGET_DIR: "C:\\SageCaches\\target" },
    npmInvocation,
  });

  for (const plan of [developer, integration]) {
    assert.deepEqual(
      plan.prerequisiteGroups.flatMap(({ commands }) =>
        commands.map(({ args }) => args.slice(-2)),
      ),
      [["run", "build"]],
    );
    assert.ok(
      plan.groups.some(({ cargoTargetDir }) => cargoTargetDir !== undefined),
    );
  }
  assert.equal(integration.groups[0].concurrency, 3);
  assert.ok(
    !integration.groups[0].commands.some(
      ({ label }) => label === "frontend:build",
    ),
  );
});

// origin: check-lanes.node-test.mjs:213 "integration plan preserves every frontend phase and serializes Rust tools" -> check-lanes.portable.node-test.mjs
test("integration plan preserves every frontend phase and serializes Rust tools", () => {
  const plan = createIntegrationPlan({
    platform: "win32",
    repositoryRoot,
    rustEnvironment: { CARGO_TARGET_DIR: "C:\\SageCaches\\target" },
    npmInvocation: {
      command: "C:\\node\\node.exe",
      argsPrefix: ["C:\\node\\node_modules\\npm\\bin\\npm-cli.js"],
    },
  });

  assert.deepEqual(
    plan.groups[0].commands.map(({ label }) => label),
    [
      "frontend:format",
      "frontend:lint",
      "frontend:vitest",
      "frontend:budgets",
      "frontend:policy",
    ],
  );
  assert.deepEqual(
    plan.prerequisiteGroups.flatMap(({ commands }) =>
      commands.map(({ label }) => label),
    ),
    ["frontend:build"],
  );
  assert.equal(plan.groups[0].concurrency, 3);
  assert.equal(plan.groups[1].concurrency, 1);
  assert.equal(
    plan.groups[0].timeoutMs,
    VALIDATION_DEADLINES_MS.integrationFrontendLane,
  );
  assert.equal(
    plan.groups[1].timeoutMs,
    VALIDATION_DEADLINES_MS.integrationRustLane,
  );
  assert.deepEqual(
    plan.groups[1].commands.map(({ args }) => args),
    [
      ["fmt", "--manifest-path", "src-tauri/Cargo.toml", "--", "--check"],
      [
        "clippy",
        "--manifest-path",
        "src-tauri/Cargo.toml",
        "--all-targets",
        "--all-features",
        "--",
        "-D",
        "warnings",
      ],
      [
        "test",
        "--manifest-path",
        "src-tauri/Cargo.toml",
        "--all-features",
        "--",
        "--test-threads=1",
      ],
    ],
  );
  assert.equal(plan.groups[0].commands[0].command, "C:\\node\\node.exe");
  assert.match(plan.groups[0].commands[0].args[0], /npm-cli\.js$/u);
  assert.equal(plan.groups[1].commands[0].command, "cargo.exe");
  assert.deepEqual(
    plan.groups[1].commands,
    createRustValidationCommands({
      labelPrefix: "integration",
      platform: "win32",
      repositoryRoot,
      rustEnvironment: { CARGO_TARGET_DIR: "C:\\SageCaches\\target" },
    }),
  );
});

// origin: check-lanes.node-test.mjs:288 "resolved npm invocation executes without a shell command string" -> check-lanes.portable.node-test.mjs
test("resolved npm invocation executes without a shell command string", async () => {
  const invocation = resolveNpmInvocation();
  let output = "";
  const stdout = new Writable({
    write(chunk, _encoding, callback) {
      output += chunk.toString();
      callback();
    },
  });
  await executeCommand(
    {
      label: "npm-version",
      command: invocation.command,
      args: [...invocation.argsPrefix, "--version"],
      cwd: path.resolve("."),
      timeoutMs: 5_000,
      terminationGraceMs: 200,
      settlementTimeoutMs: 1_500,
      maximumLineBytes: 1_024,
    },
    new AbortController().signal,
    { stdout, stderr: discardStream() },
  );
  assert.match(output, /\d+\.\d+\.\d+/u);
});

// origin: check-lanes.node-test.mjs:336 "release lane fails closed and preserves conservative validation order" -> check-lanes.portable.node-test.mjs
test("release lane fails closed and preserves conservative validation order", () => {
  assert.throws(() => parseReleaseArguments([]), /explicit mode/);
  assert.throws(
    () => parseReleaseArguments(["--package-campaign"]),
    /missing required package arguments/,
  );
  assert.throws(
    () =>
      parseReleaseArguments([
        "--validation-only",
        "--",
        "--campaign-id",
        "unexpected",
      ]),
    /does not accept package arguments/,
  );

  const options = parseReleaseArguments(["--validation-only", "--dry-run"]);
  const plan = createReleasePlan(options, {
    platform: "linux",
    repositoryRoot,
    rustEnvironment: {},
  });
  assert.equal(plan.groups.length, 1);
  assert.equal(plan.groups[0].concurrency, 1);
  assert.equal(plan.groups[0].timeoutMs, VALIDATION_DEADLINES_MS.releaseLane);
  assert.deepEqual(
    plan.groups[0].commands.map(({ label }) => label),
    [
      "release:frontend-check",
      "release:rust-format",
      "release:rust-clippy",
      "release:rust-tests",
    ],
  );
  assert.deepEqual(plan.groups[0].commands[3].args.slice(-2), [
    "--",
    "--test-threads=1",
  ]);
  assert.deepEqual(
    plan.groups[0].commands.slice(1),
    createRustValidationCommands({
      labelPrefix: "release",
      platform: "linux",
      repositoryRoot,
      rustEnvironment: {},
    }),
  );
});

// origin: check-lanes.node-test.mjs:386 "release package mode hands exact validated arguments to the existing campaign" -> check-lanes.portable.node-test.mjs
test("release package mode hands exact validated arguments to the existing campaign", () => {
  const packageArguments = [
    "--source-root",
    "C:\\source",
    "--campaign-id",
    "beta-1",
    "--rust-toolchain-root",
    "C:\\rust",
    "--cargo-home",
    "C:\\cargo",
    "--node",
    "C:\\node\\node.exe",
    "--npm-package-root",
    "C:\\npm",
    "--git-root",
    "C:\\git",
    "--wix-tools-root",
    "C:\\wix",
    "--nsis-tools-root",
    "C:\\nsis",
    "--msvc-tools-root",
    "C:\\msvc",
    "--windows-sdk-root",
    "C:\\sdk",
    "--windows-sdk-version",
    "10.0.26100.0",
    "--seven-zip",
    "C:\\7zip\\7z.exe",
    "--installed-smoke-output",
    "C:\\evidence",
    "--installed-smoke-signer-thumbprint",
    "0123456789abcdef0123456789abcdef01234567",
  ];
  const options = parseReleaseArguments([
    "--package-campaign",
    "--dry-run",
    "--",
    ...packageArguments,
  ]);
  const plan = createReleasePlan(options, {
    platform: "win32",
    repositoryRoot,
    rustEnvironment: {},
    npmInvocation: {
      command: "C:\\node\\node.exe",
      argsPrefix: ["C:\\node\\node_modules\\npm\\bin\\npm-cli.js"],
    },
  });
  const command = plan.groups[0].commands.at(-1);
  assert.equal(command.label, "release:package-and-installed-smoke-handoff");
  assert.equal(command.command, process.execPath);
  assert.deepEqual(command.args, [
    "scripts/collect-ade0006a-package.mjs",
    ...packageArguments,
  ]);
});

// origin: check-lanes.node-test.mjs:443 "Rust environment uses explicit paths and only enables detected sccache" -> check-lanes.portable.node-test.mjs
test("Rust environment uses explicit paths and only enables detected sccache", () => {
  assert.deepEqual(
    buildRustEnvironment({
      platform: "linux",
      environment: {
        SAGE_CARGO_TARGET_DIR: "/cache/target",
        SAGE_SCCACHE_DIR: "/cache/sccache",
      },
      repositoryRoot: "/workspace/sage",
      sccachePath: "/usr/bin/sccache",
      windowsDDriveAvailable: false,
    }),
    {
      CARGO_TARGET_DIR: "/cache/target",
      RUSTC_WRAPPER: "/usr/bin/sccache",
      SCCACHE_CACHE_SIZE: "20G",
      SCCACHE_DIR: "/cache/sccache",
    },
  );
  const windowsEnvironment = buildRustEnvironment({
    platform: "win32",
    environment: {},
    repositoryRoot,
    sccachePath: undefined,
    windowsDDriveAvailable: true,
  });
  assert.match(
    windowsEnvironment.CARGO_TARGET_DIR,
    /^C:\\SageCaches\\cargo-target\\sage-[0-9a-f]{12}$/u,
  );
  const first = buildRustEnvironment({
    platform: "win32",
    environment: {},
    repositoryRoot: "C:\\one\\same-name",
    windowsDDriveAvailable: true,
  });
  const second = buildRustEnvironment({
    platform: "win32",
    environment: {},
    repositoryRoot: "C:\\two\\same-name",
    windowsDDriveAvailable: true,
  });
  assert.notEqual(first.CARGO_TARGET_DIR, second.CARGO_TARGET_DIR);
});

// origin: check-lanes.node-test.mjs:488 "Cargo target lease excludes a second Node process and releases cleanly" -> check-lanes.portable.node-test.mjs
test("Cargo target lease excludes a second Node process and releases cleanly", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-lease-"));
  const target = path.join(root, "target");
  const moduleUrl = pathToFileURL(
    path.resolve("scripts/lib/check-cargo-lease.mjs"),
  ).href;
  const holderSource = `
    const { acquireCargoTargetLease } = await import(${JSON.stringify(moduleUrl)});
    const lease = await acquireCargoTargetLease(process.argv[1]);
    process.stdout.write("READY\\n");
    process.stdin.once("data", async () => { await lease.release(); process.exit(0); });
    setTimeout(async () => { await lease.release(); process.exit(0); }, 5000);
  `;
  const holder = spawn(
    process.execPath,
    ["--input-type=module", "-e", holderSource, target],
    {
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    },
  );
  try {
    await new Promise((resolve, reject) => {
      const timer = setTimeout(
        () => reject(new Error("holder readiness timeout")),
        2_000,
      );
      holder.once("error", reject);
      holder.stdout.once("data", (chunk) => {
        clearTimeout(timer);
        assert.equal(chunk.toString(), "READY\n");
        resolve();
      });
    });
    const contenderSource = `
      const { acquireCargoTargetLease } = await import(${JSON.stringify(moduleUrl)});
      try { await acquireCargoTargetLease(process.argv[1]); process.exit(9); }
      catch (error) { process.stderr.write(error.code ?? "unknown"); process.exit(2); }
    `;
    let contenderError = "";
    await assert.rejects(
      executeCommand(
        {
          label: "cargo-lease-contender",
          command: process.execPath,
          args: ["--input-type=module", "-e", contenderSource, target],
          cwd: root,
          timeoutMs: 2_000,
          terminationGraceMs: 200,
          settlementTimeoutMs: 1_000,
          maximumLineBytes: 1_024,
        },
        new AbortController().signal,
        {
          stdout: discardStream(),
          stderr: new Writable({
            write(chunk, _encoding, callback) {
              contenderError += chunk.toString();
              callback();
            },
          }),
        },
      ),
      /exit 2/,
    );
    assert.match(contenderError, /cargo-target-locked/u);
    holder.stdin.end("release\n");
    await new Promise((resolve) => holder.once("close", resolve));
    assert.equal(await processExists(holder.pid), false);
    await assert.rejects(readFile(await cargoLeasePath(target)), /ENOENT/u);
  } finally {
    if (await processExists(holder.pid)) holder.kill("SIGKILL");
    await rm(root, { recursive: true, force: true });
  }
});

// origin: check-lanes.node-test.mjs:564 "physical Cargo target aliases contend on one lease" -> check-lanes.portable.node-test.mjs
test("physical Cargo target aliases contend on one lease", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-alias-"));
  const physical = path.join(root, "physical");
  const alias = path.join(root, "alias");
  const physicalTarget = path.join(physical, "new", "target");
  const aliasTarget = path.join(alias, "new", "target");
  await mkdir(physical);
  try {
    await symlink(
      physical,
      alias,
      process.platform === "win32" ? "junction" : "dir",
    );
    assert.equal(
      await canonicalCargoTarget(physical, { repositoryRoot: root }),
      await canonicalCargoTarget(alias, { repositoryRoot: root }),
    );
    const lease = await acquireCargoTargetLease(physicalTarget, {
      repositoryRoot: root,
    });
    try {
      await assert.rejects(
        acquireCargoTargetLease(aliasTarget, { repositoryRoot: root }),
        (error) => error?.code === "cargo-target-locked",
      );
    } finally {
      await lease.release();
    }
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// origin: check-lanes.node-test.mjs:597 "Cargo lease namespace survives nested target materialization" -> check-lanes.portable.node-test.mjs
test("Cargo lease namespace survives nested target materialization", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-materialize-"));
  const target = path.join(root, "new", "nested", "target");
  let holder;
  let contender;
  try {
    holder = await acquireCargoTargetLease(target, { repositoryRoot: root });
    await mkdir(target, { recursive: true });
    await assert.rejects(
      async () => {
        contender = await acquireCargoTargetLease(target, {
          repositoryRoot: root,
        });
      },
      (error) => error?.code === "cargo-target-locked",
    );
  } finally {
    await contender?.release();
    await holder?.release();
    await rm(root, { recursive: true, force: true });
  }
});

// origin: check-lanes.node-test.mjs:620 "nonexistent Cargo target canonicalizes its physical ancestor" -> check-lanes.portable.node-test.mjs
test("nonexistent Cargo target canonicalizes its physical ancestor", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-canonical-"));
  const physical = path.join(root, "physical");
  const alias = path.join(root, "alias");
  await mkdir(physical);
  try {
    await symlink(
      physical,
      alias,
      process.platform === "win32" ? "junction" : "dir",
    );
    assert.equal(
      await canonicalCargoTarget(path.join(physical, "new", "target"), {
        repositoryRoot: root,
      }),
      await canonicalCargoTarget(path.join(alias, "new", "target"), {
        repositoryRoot: root,
      }),
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// origin: check-lanes.node-test.mjs:644 "Darwin lease identity folds case for default case-insensitive targets" -> check-lanes.portable.node-test.mjs
test("Darwin lease identity folds case for default case-insensitive targets", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-darwin-case-"));
  try {
    assert.equal(
      await cargoLeasePath(path.join(root, "Target", "New"), {
        repositoryRoot: root,
        platform: "darwin",
      }),
      await cargoLeasePath(path.join(root, "target", "new"), {
        repositoryRoot: root,
        platform: "darwin",
      }),
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// origin: check-lanes.node-test.mjs:687 "Cargo lease acquisition obeys its deadline without a partial owner" -> check-lanes.portable.node-test.mjs
test("Cargo lease acquisition obeys its deadline without a partial owner", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-lease-timeout-"));
  const target = path.join(root, "target");
  const controller = new AbortController();
  const timer = setTimeout(
    () => controller.abort(new Error("lane deadline")),
    25,
  );
  try {
    await assert.rejects(
      acquireCargoTargetLease(target, {
        repositoryRoot: root,
        signal: controller.signal,
        operations: { mkdir: async () => new Promise(() => {}) },
      }),
      (error) => error?.code === "cargo-target-lease-timeout",
    );
    await assert.rejects(
      readFile(await cargoLeasePath(target, { repositoryRoot: root })),
      /ENOENT/u,
    );
  } finally {
    clearTimeout(timer);
    await rm(root, { recursive: true, force: true });
  }
});

// origin: check-lanes.node-test.mjs:714 "unconfirmed Cargo command settlement quarantines the lease and denies a new process" -> check-lanes.portable.node-test.mjs
test("unconfirmed Cargo command settlement quarantines the lease and denies a new process", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-quarantine-"));
  const target = path.join(root, "target");
  const moduleUrl = pathToFileURL(
    path.resolve("scripts/lib/check-cargo-lease.mjs"),
  ).href;
  try {
    await assert.rejects(
      runCommandGroups(
        [
          {
            label: "rust-quarantine",
            concurrency: 2,
            timeoutMs: 2_000,
            cargoTargetDir: target,
            repositoryRoot: root,
            commands: [
              { label: "aborted-sibling", timeoutMs: 1_000 },
              { label: "unconfirmed", timeoutMs: 1_000 },
            ],
          },
        ],
        {
          execute: async ({ label }, signal) => {
            if (label === "aborted-sibling") {
              await new Promise((resolve) =>
                signal.addEventListener("abort", resolve, { once: true }),
              );
              throw new CheckProcessError("sibling settled after abort", {
                code: "command-aborted",
                settled: true,
              });
            }
            throw new CheckProcessError("descendant settlement unconfirmed", {
              code: "command-timeout",
              settled: false,
            });
          },
          executeWindowsCargo: async ({ label }, signal) => {
            if (label === "aborted-sibling") {
              await new Promise((resolve) =>
                signal.addEventListener("abort", resolve, { once: true }),
              );
              throw new CheckProcessError("sibling settled after abort", {
                code: "command-aborted",
                settled: true,
              });
            }
            throw new CheckProcessError("descendant settlement unconfirmed", {
              code: "command-timeout",
              settled: false,
            });
          },
        },
      ),
      (error) => error?.code === "cargo-target-settlement-unconfirmed",
    );
    const quarantine = JSON.parse(
      await readFile(await cargoLeasePath(target), "utf8"),
    );
    assert.equal(quarantine.quarantined, true);
    assert.equal(quarantine.failureCode, "command-timeout");

    const contenderSource = `
      const { acquireCargoTargetLease } = await import(${JSON.stringify(moduleUrl)});
      try { await acquireCargoTargetLease(process.argv[1]); process.exit(9); }
      catch (error) { process.stderr.write(error.code ?? "unknown"); process.exit(2); }
    `;
    let contenderError = "";
    await assert.rejects(
      executeCommand(
        {
          label: "quarantined-target-contender",
          command: process.execPath,
          args: ["--input-type=module", "-e", contenderSource, target],
          cwd: root,
          timeoutMs: 2_000,
          terminationGraceMs: 200,
          settlementTimeoutMs: 1_000,
          maximumLineBytes: 1_024,
          maximumOutputBytes: 4_096,
        },
        new AbortController().signal,
        {
          stdout: discardStream(),
          stderr: new Writable({
            write(chunk, _encoding, callback) {
              contenderError += chunk.toString();
              callback();
            },
          }),
        },
      ),
      /exit 2/,
    );
    assert.match(contenderError, /cargo-target-locked/u);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// origin: check-lanes.node-test.mjs:885 "unavailable Windows Job proof quarantines Cargo" -> check-lanes.portable.node-test.mjs
test("unavailable Windows Job proof quarantines Cargo", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-job-proof-"));
  const target = path.join(root, "target");
  try {
    await assert.rejects(
      runCommandGroups(
        [
          {
            label: "rust-proof-failure",
            concurrency: 1,
            timeoutMs: 2_000,
            cargoTargetDir: target,
            repositoryRoot: root,
            commands: [{ label: "proof-failure", timeoutMs: 1_000 }],
          },
        ],
        {
          // Portable: the runner selects execute on POSIX and
          // executeWindowsCargo on Windows; both paths must prove the same
          // quarantine instead of silently passing on one host.
          execute: async () => {
            throw new CheckProcessError("proof unavailable", {
              code: "windows-job-proof-unavailable",
              settled: false,
            });
          },
          executeWindowsCargo: async () => {
            throw new CheckProcessError("proof unavailable", {
              code: "windows-job-proof-unavailable",
              settled: false,
            });
          },
        },
      ),
      (error) => error?.code === "cargo-target-settlement-unconfirmed",
    );
    const quarantine = JSON.parse(
      await readFile(
        await cargoLeasePath(target, { repositoryRoot: root }),
        "utf8",
      ),
    );
    assert.equal(quarantine.quarantined, true);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// origin: check-lanes.node-test.mjs:1312 "bounded runner caps concurrency and aborts sibling work on failure" -> check-lanes.portable.node-test.mjs
test("bounded runner caps concurrency and aborts sibling work on failure", async () => {
  let active = 0;
  let maximumActive = 0;
  let observedAbort = false;
  const commands = ["fail", "sibling", "never"].map((label) => ({
    label,
    timeoutMs: 1_000,
  }));

  await assert.rejects(
    runCommandGroups(
      [{ label: "test", concurrency: 2, timeoutMs: 2_000, commands }],
      {
        execute: async ({ label }, signal) => {
          active += 1;
          maximumActive = Math.max(maximumActive, active);
          try {
            if (label === "fail") {
              await new Promise((resolve) => setTimeout(resolve, 10));
              throw new Error("expected failure");
            }
            await new Promise((resolve) => {
              const timer = setTimeout(resolve, 5_000);
              signal.addEventListener(
                "abort",
                () => {
                  observedAbort = true;
                  clearTimeout(timer);
                  resolve();
                },
                { once: true },
              );
            });
          } finally {
            active -= 1;
          }
        },
      },
    ),
    /expected failure/,
  );
  assert.equal(maximumActive, 2);
  assert.equal(observedAbort, true);
});

// origin: check-lanes.node-test.mjs:1411 "real command timeout terminates its spawned process tree without an orphan" -> check-lanes.portable.node-test.mjs
test("real command timeout terminates its spawned process tree without an orphan", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-timeout-"));
  const descendantPidPath = path.join(root, "descendant.pid");
  const descendantSource =
    "process.on('SIGTERM', () => {}); setInterval(() => {}, 1000)";
  const source = `
    const { spawn } = require("node:child_process");
    const { writeFileSync } = require("node:fs");
    const descendant = spawn(process.execPath, ["-e", ${JSON.stringify(descendantSource)}], {
      stdio: "ignore",
      windowsHide: true,
    });
    writeFileSync(process.argv[1], String(descendant.pid));
    setInterval(() => {}, 1000);
  `;
  let descendantPid;
  try {
    await assert.rejects(
      executeCommand(
        {
          label: "timeout-tree",
          command: process.execPath,
          args: ["-e", source, descendantPidPath],
          cwd: root,
          timeoutMs: 300,
          terminationGraceMs: 200,
          settlementTimeoutMs: 1_500,
          maximumLineBytes: 1_024,
          maximumOutputBytes: 4_096,
        },
        new AbortController().signal,
        { stdout: discardStream(), stderr: discardStream() },
      ),
      (error) => error?.code === "command-timeout",
    );
    descendantPid = Number.parseInt(
      await readFile(descendantPidPath, "utf8"),
      10,
    );
    assert.ok(Number.isSafeInteger(descendantPid) && descendantPid > 0);
    await waitForProcessExit(descendantPid);
  } finally {
    if (descendantPid && (await processExists(descendantPid))) {
      process.kill(descendantPid, "SIGKILL");
    }
    await rm(root, { recursive: true, force: true });
  }
});

// origin: check-lanes.node-test.mjs:1460 "leader exit cannot falsely prove descendant settlement" -> check-lanes.portable.node-test.mjs
test("leader exit cannot falsely prove descendant settlement", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-leader-exit-"));
  const descendantPidPath = path.join(root, "descendant.pid");
  const source = `
    const { spawn } = require("node:child_process");
    const { writeFileSync } = require("node:fs");
    const descendant = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], {
      stdio: ["ignore", "inherit", "inherit"],
      windowsHide: true,
    });
    writeFileSync(process.argv[1], String(descendant.pid));
    process.on("SIGTERM", () => process.exit(0));
    setInterval(() => {}, 1000);
  `;
  let descendantPid;
  try {
    await assert.rejects(
      executeCommand(
        {
          label: "leader-exit-first",
          command: process.execPath,
          args: ["-e", source, descendantPidPath],
          cwd: root,
          timeoutMs: 300,
          terminationGraceMs: 200,
          settlementTimeoutMs: 1_000,
          maximumLineBytes: 1_024,
          maximumOutputBytes: 4_096,
        },
        new AbortController().signal,
        { stdout: discardStream(), stderr: discardStream() },
      ),
      (error) => error?.code === "command-timeout" && error.settled === true,
    );
    descendantPid = Number.parseInt(
      await readFile(descendantPidPath, "utf8"),
      10,
    );
    await waitForProcessExit(descendantPid);
  } finally {
    await cleanProcessFixture(root, descendantPidPath, descendantPid);
  }
});

// origin: check-lanes.node-test.mjs:1556 "newline-free output is bounded and terminates the offending command" -> check-lanes.portable.node-test.mjs
test("newline-free output is bounded and terminates the offending command", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-output-"));
  let emittedBytes = 0;
  const output = new Writable({
    write(chunk, _encoding, callback) {
      emittedBytes += chunk.byteLength;
      callback();
    },
  });
  try {
    await assert.rejects(
      executeCommand(
        {
          label: "output-overflow",
          command: process.execPath,
          args: [
            "-e",
            "process.stdout.write('x'.repeat(70000)); setInterval(() => {}, 1000)",
          ],
          cwd: root,
          timeoutMs: 5_000,
          terminationGraceMs: 200,
          settlementTimeoutMs: 1_500,
          maximumLineBytes: 1_024,
          maximumOutputBytes: 4_096,
        },
        new AbortController().signal,
        { stdout: output, stderr: discardStream() },
      ),
      (error) => error?.code === "output-overflow",
    );
    assert.ok(emittedBytes <= 1_200, `overflow emitted ${emittedBytes} bytes`);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

function discardStream() {
  return new Writable({
    write(_chunk, _encoding, callback) {
      callback();
    },
  });
}

async function processExists(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    if (error.code === "ESRCH") return false;
    throw error;
  }
}

async function waitForProcessExit(pid, timeoutMs = 2_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (!(await processExists(pid))) return;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  assert.fail(`process ${pid} remained alive after the runner settled`);
}

async function cleanProcessFixture(root, pidPath, descendantPid) {
  if (!descendantPid) {
    try {
      const candidate = Number.parseInt(await readFile(pidPath, "utf8"), 10);
      if (Number.isSafeInteger(candidate) && candidate > 0) {
        descendantPid = candidate;
      }
    } catch (error) {
      if (error?.code !== "ENOENT") throw error;
    }
  }
  if (descendantPid && (await processExists(descendantPid))) {
    try {
      process.kill(descendantPid, "SIGKILL");
    } catch (error) {
      if (error?.code !== "ESRCH") throw error;
    }
    await waitForProcessExit(descendantPid);
  }
  await rm(root, {
    recursive: true,
    force: true,
    maxRetries: 5,
    retryDelay: 100,
  });
}
