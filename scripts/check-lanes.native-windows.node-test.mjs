import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import {
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { Writable } from "node:stream";
import test from "node:test";
import { pathToFileURL } from "node:url";

import {
  acquireCargoTargetLease,
  cargoLeasePath,
} from "./lib/check-cargo-lease.mjs";
import { runCommandGroups } from "./lib/check-process.mjs";
import { executeWindowsCargoJobCommand } from "./lib/check-windows-cargo-job.mjs";
import { MAX_STALL_IMAGE_BYTES } from "./lib/check-windows-cargo-protocol.mjs";

async function runVctipTeardownAdapterFixture({
  rootExitCode,
  childLifetimeMs,
  observationMode = 0,
  naturalExitTrigger = false,
}) {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-vctip-adapter-"));
  const childPidPath = path.join(root, "child.pid");
  const rootHelper = path.join(root, "root.cjs");
  const driver = path.join(root, "driver.ps1");
  const childSource = JSON.stringify(
    naturalExitTrigger
      ? `const fs=require("node:fs");const path=require("node:path");const ready=process.argv[1];const trigger=process.argv[2];const watcher=fs.watch(path.dirname(trigger),(_event,name)=>{if(name===path.basename(trigger)&&fs.existsSync(trigger)){watcher.close();process.exit(0)}});fs.writeFileSync(ready,"ready");`
      : `require("node:fs").writeFileSync(process.argv[1], "ready");setTimeout(() => process.exit(0), ${childLifetimeMs})`,
  );
  try {
    await writeFile(
      rootHelper,
      `const {spawn}=require("node:child_process");const fs=require("node:fs");const childPidPath=process.argv[2];process.on("uncaughtException",(error)=>{fs.writeFileSync(childPidPath+".error",String(error&&error.stack||error));process.exit(70)});const childReadyPath=childPidPath+".ready";const naturalExitTriggerPath=process.argv[3]||"";const child=spawn(process.execPath,["-e",${childSource},childReadyPath,naturalExitTriggerPath],{detached:true,stdio:"ignore",windowsHide:true});fs.writeFileSync(childPidPath,String(child.pid));const deadline=Date.now()+2000;(function waitForChildReady(){if(fs.existsSync(childReadyPath))process.exit(${rootExitCode});if(Date.now()>=deadline)throw new Error("child-ready handshake timed out");setTimeout(waitForChildReady,10);})();`,
    );
    await writeFile(
      driver,
      [
        `param([string]$NativePath,[string]$NodePath,[string]$RootHelper,[string]$ChildPidPath,[string]$FixtureRoot,[int]$ObservationMode,[ValidateSet(0,1)][int]$NaturalExitTrigger)`,
        `$ErrorActionPreference = 'Stop'`,
        `[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)`,
        `$source = "#define SAGE_VCTIP_JOB_TESTS" + [Environment]::NewLine + [IO.File]::ReadAllText($NativePath)`,
        `Add-Type -TypeDefinition $source -ErrorAction Stop`,
        `$stdout = $null; $stderr = $null; $job = $null`,
        `try {`,
        `  $triggerPath = if ($NaturalExitTrigger -eq 1) { $ChildPidPath + '.trigger' } else { $null }`,
        `  $job = [SageValidation.CargoJob]::Start($NodePath, @($RootHelper,$ChildPidPath,([string]$triggerPath)), $FixtureRoot, @(('PATH=' + $env:PATH), ('SystemRoot=' + $env:SystemRoot)), 4, [ref]$stdout, [ref]$stderr)`,
        `  $deadline = [DateTime]::UtcNow.AddSeconds(3)`,
        `  while ($job.RootRunning -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 5 }`,
        `  if ($job.RootRunning -or -not (Test-Path -LiteralPath $ChildPidPath) -or -not (Test-Path -LiteralPath ($ChildPidPath + '.ready'))) { $rootError = if (Test-Path -LiteralPath ($ChildPidPath + '.error')) { [IO.File]::ReadAllText($ChildPidPath + '.error') } else { '' }; $rootExit = if ($job.RootRunning) { 'running' } else { [string]$job.ExitCode() }; throw ('root/child fixture did not become ready (root: {0}, child.pid: {1}, ready: {2}) {3}' -f $rootExit, (Test-Path -LiteralPath $ChildPidPath), (Test-Path -LiteralPath ($ChildPidPath + '.ready')), $rootError) }`,
        `  $member = [uint32]([IO.File]::ReadAllText($ChildPidPath).Trim())`,
        `  $adapter = $job.GetType().GetMethod('EnableVctipTeardownTestAdapter', [Reflection.BindingFlags]::Instance -bor [Reflection.BindingFlags]::NonPublic)`,
        `  if ($null -eq $adapter) { throw 'compiled VCTIP test adapter was unavailable' }`,
        `  [void]$adapter.Invoke($job, @($member, [uint32]$ObservationMode, $triggerPath))`,
        `  $teardown = $job.TryTeardownVerifiedVctip()`,
        `  $attemptsProperty = $job.GetType().GetProperty('VctipTeardownTestTerminationAttempts', [Reflection.BindingFlags]::Instance -bor [Reflection.BindingFlags]::NonPublic)`,
        `  if ($null -eq $attemptsProperty) { throw 'compiled VCTIP termination observation was unavailable' }`,
        `  $hooksProperty = $job.GetType().GetProperty('VctipTeardownTestNaturalExitHooks', [Reflection.BindingFlags]::Instance -bor [Reflection.BindingFlags]::NonPublic)`,
        `  $naturalExitProperty = $job.GetType().GetProperty('VctipTeardownTestNaturalExitCode', [Reflection.BindingFlags]::Instance -bor [Reflection.BindingFlags]::NonPublic)`,
        `  $emptyJobProperty = $job.GetType().GetProperty('VctipTeardownTestJobProcessCount', [Reflection.BindingFlags]::Instance -bor [Reflection.BindingFlags]::NonPublic)`,
        `  if ($null -eq $hooksProperty -or $null -eq $naturalExitProperty -or $null -eq $emptyJobProperty) { throw 'compiled VCTIP native observation was unavailable' }`,
        `  while ($job.ActiveProcesses() -ne 0 -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 5 }`,
        `  [ordered]@{ exitCode=$job.ExitCode(); active=[int]$job.ActiveProcesses(); emptyJobMembers=[int]$emptyJobProperty.GetValue($job); terminationAttempts=[int]$attemptsProperty.GetValue($job); naturalExitHooks=[int]$hooksProperty.GetValue($job); naturalExitCode=[int]$naturalExitProperty.GetValue($job); teardown=$teardown } | ConvertTo-Json -Compress -Depth 4`,
        `} finally { if ($null -ne $stdout) { $stdout.Dispose() }; if ($null -ne $stderr) { $stderr.Dispose() }; if ($null -ne $job) { $job.Dispose() } }`,
      ].join("\n"),
    );
    const nativePath = path.resolve("scripts/lib/WindowsCargoJob.Native.cs");
    return JSON.parse(
      execFileSync(
        "powershell.exe",
        [
          "-NoLogo",
          "-NoProfile",
          "-NonInteractive",
          "-ExecutionPolicy",
          "Bypass",
          "-File",
          driver,
          nativePath,
          process.execPath,
          rootHelper,
          childPidPath,
          root,
          String(observationMode),
          naturalExitTrigger ? "1" : "0",
        ],
        { encoding: "utf8", timeout: 15_000, windowsHide: true },
      ).trim(),
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

async function runVctipProductionWrapperFixture({
  rootExitCode,
  testDefine = "",
}) {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-vctip-wrapper-"));
  const library = path.join(root, "scripts", "lib");
  const rootHelper = path.join(root, "root.cjs");
  try {
    await mkdir(library, { recursive: true });
    for (const name of [
      "windows-cargo-job.ps1",
      "WindowsCargoJob.psm1",
      "check-child-process.mjs",
      "process-liveness.mjs",
      "check-windows-cargo-protocol.mjs",
      "check-windows-cargo-job.mjs",
    ]) {
      await copyFile(
        path.resolve("scripts/lib", name),
        path.join(library, name),
      );
    }
    const nativeSource = await readFile(
      path.resolve("scripts/lib/WindowsCargoJob.Native.cs"),
      "utf8",
    );
    await writeFile(
      path.join(library, "WindowsCargoJob.Native.cs"),
      `#define SAGE_VCTIP_JOB_TESTS\n${testDefine}${nativeSource}`,
    );
    await writeFile(
      rootHelper,
      `const {spawn}=require("node:child_process");const fs=require("node:fs");const childReadyPath=process.argv[2]+".ready";const child=spawn(process.execPath,["-e","require('node:fs').writeFileSync(process.argv[1],'ready');setInterval(()=>{},1000)",childReadyPath],{detached:true,stdio:"ignore",windowsHide:true});const deadline=Date.now()+2000;(function waitForChildReady(){if(fs.existsSync(childReadyPath))process.exit(${rootExitCode});if(Date.now()>=deadline)throw new Error("child-ready handshake timed out");setTimeout(waitForChildReady,10);})();`,
    );
    const { executeWindowsCargoJobCommand } = await import(
      `${pathToFileURL(path.join(library, "check-windows-cargo-job.mjs")).href}?${Date.now()}-${Math.random()}`
    );
    let observed;
    await assert.rejects(
      executeWindowsCargoJobCommand(
        {
          label: "vctip-production-wrapper",
          command: process.execPath,
          args: [rootHelper, path.join(root, "child.pid")],
          cwd: root,
          timeoutMs: 5_000,
          terminationGraceMs: 200,
          settlementTimeoutMs: 2_000,
          maximumLineBytes: 1_024,
          maximumOutputBytes: 4_096,
        },
        new AbortController().signal,
        { stdout: discardStream(), stderr: discardStream() },
      ),
      (error) => {
        observed = error;
        return true;
      },
    );
    return observed;
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

function windowsCargoJobStallReport(rootRunning, memberCount) {
  const modulePath = path.resolve("scripts/lib/WindowsCargoJob.psm1");
  const script = [
    "$ErrorActionPreference = 'Stop'",
    `$module = Import-Module '${modulePath.replaceAll("'", "''")}' -PassThru -Force`,
    `$names = [string[]]@(@(('x' * 251) + '.exe') * ${memberCount})`,
    `& $module { param($names) New-CargoJobStallReport $${rootRunning} $names | ConvertTo-Json -Compress -Depth 4 } $names`,
  ].join("; ");
  return JSON.parse(
    execFileSync(
      "powershell.exe",
      ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script],
      { encoding: "utf8", timeout: 30_000, windowsHide: true },
    ),
  );
}

test("Windows Cargo Job stall report keeps member names within the proof budget", () => {
  const full = windowsCargoJobStallReport(true, 32);
  assert.equal(full.rootRunning, true);
  assert.ok(full.omittedMembers > 0);
  assert.equal(full.memberImages.length + full.omittedMembers, 32);
  assert.ok(
    Buffer.byteLength(JSON.stringify(full.memberImages)) <=
      MAX_STALL_IMAGE_BYTES,
  );
  assert.ok(
    Buffer.byteLength(
      JSON.stringify([...full.memberImages, full.memberImages[0]]),
    ) > MAX_STALL_IMAGE_BYTES,
  );
  assert.deepEqual(windowsCargoJobStallReport(false, 0), {
    rootRunning: false,
    memberImages: [],
    omittedMembers: 0,
  });
});

// origin: Q01_VCTIP_TEARDOWN "exact identity positive teardown"
test("compiled VCTIP test adapter preserves root exit 7 after verified teardown", async () => {
  const result = await runVctipTeardownAdapterFixture({
    rootExitCode: 7,
    childLifetimeMs: 30_000,
  });
  assert.equal(result.exitCode, 7);
  assert.equal(result.active, 0);
  assert.equal(result.emptyJobMembers, 0);
  assert.equal(result.terminationAttempts, 1);
  assert.equal(result.teardown.kind, "verified-msvc-vctip");
  assert.equal(result.teardown.terminationExitCodeRequested, 0x53475654);
  assert.equal(result.teardown.terminationExitCodeObserved, 0x53475654);
});

// origin: Q01_VCTIP_TEARDOWN "production wrapper preserves root failure receipt"
test("production Windows Cargo wrapper exposes exit 7 and verified VCTIP receipt", async () => {
  const error = await runVctipProductionWrapperFixture({ rootExitCode: 7 });
  assert.equal(error.code, "command-failed");
  assert.equal(error.settled, true);
  assert.equal(error.exitCode, 7);
  assert.equal(error.teardown.kind, "verified-msvc-vctip");
  assert.equal(error.teardown.terminationExitCodeObserved, 0x53475654);
});

// origin: Q01_VCTIP_TEARDOWN "post-termination observations remain explicit"
test("production Windows Cargo wrapper reports unconfirmed VCTIP termination observations", async () => {
  for (const [define, observation] of [
    ["#define SAGE_VCTIP_JOB_TEST_WAIT_FAILED\n", "wait-failed"],
    ["#define SAGE_VCTIP_JOB_TEST_WAIT_TIMEOUT\n", "wait-timeout"],
    ["#define SAGE_VCTIP_JOB_TEST_EXIT_UNCONFIRMED\n", "exit-code-unconfirmed"],
  ]) {
    const error = await runVctipProductionWrapperFixture({
      rootExitCode: 0,
      testDefine: define,
    });
    assert.equal(error.code, "windows-job-vctip-termination-unconfirmed");
    assert.equal(error.settled, true);
    assert.equal(error.exitCode, 0);
    assert.equal(error.teardown.kind, "msvc-vctip-termination-unconfirmed");
    assert.equal(error.teardown.terminationObservation, observation);
    assert.equal(error.teardown.terminationExitCodeRequested, 0x53475654);
  }
});

// origin: Q01_VCTIP_TEARDOWN "retained handle race leaves ambient process alone"
test("VCTIP retained-handle revalidation refuses a disappearing Job member", async () => {
  const ambient = spawn(
    process.execPath,
    ["-e", "setInterval(() => {}, 1000)"],
    {
      stdio: "ignore",
      windowsHide: true,
    },
  );
  try {
    const result = await runVctipTeardownAdapterFixture({
      rootExitCode: 0,
      childLifetimeMs: 30_000,
      naturalExitTrigger: true,
    });
    assert.equal(result.active, 0);
    assert.equal(result.emptyJobMembers, 0);
    assert.equal(result.teardown, null);
    assert.equal(result.terminationAttempts, 0);
    assert.equal(result.naturalExitHooks, 1);
    assert.equal(result.naturalExitCode, 0);
    assert.equal(await processExists(ambient.pid), true);
  } finally {
    if (ambient.pid && (await processExists(ambient.pid))) {
      process.kill(ambient.pid, "SIGKILL");
      await waitForProcessExit(ambient.pid);
    }
  }
});

// origin: check-lanes.node-test.mjs:314 "Windows Cargo Job preserves Cargo proxy command semantics" -> check-lanes.native-windows.node-test.mjs
test("Windows Cargo Job preserves Cargo proxy command semantics", async () => {
  // Instrumentation for the Cargo proxy stall, removed with its fix: with
  // SAGE_CARGO_PROXY_TRACE set, the same bounded command runs 20 more times
  // and each call's elapsed time is printed, so one run shows the
  // distribution and is more likely to catch a stall with its diagnostics.
  const runs = process.env.SAGE_CARGO_PROXY_TRACE ? 21 : 1;
  const elapsed = [];
  try {
    for (let run = 0; run < runs; run += 1) {
      const captured = { stdout: "", stderr: "" };
      const capture = (stream) =>
        new Writable({
          write(chunk, _encoding, callback) {
            captured[stream] = (
              captured[stream] + chunk.toString("utf8")
            ).slice(0, 4_096);
            callback();
          },
        });
      const started = performance.now();
      try {
        await executeWindowsCargoJobCommand(
          {
            label: "cargo-locate-project",
            command: "cargo.exe",
            args: ["locate-project", "--manifest-path", "src-tauri/Cargo.toml"],
            cwd: path.resolve("."),
            timeoutMs: 10_000,
            terminationGraceMs: 500,
            settlementTimeoutMs: 2_000,
            maximumLineBytes: 1_024,
            maximumOutputBytes: 4_096,
          },
          new AbortController().signal,
          { stdout: capture("stdout"), stderr: capture("stderr") },
        );
      } catch (error) {
        error.cargoStdout = captured.stdout;
        error.cargoStderr = captured.stderr;
        error.elapsedMs = Math.round(performance.now() - started);
        error.run = run;
        throw error;
      } finally {
        elapsed.push(Math.round(performance.now() - started));
      }
      assert.deepEqual(
        JSON.parse(captured.stdout.replace(/^\[cargo-locate-project\] /u, "")),
        { root: path.resolve("src-tauri/Cargo.toml") },
      );
    }
  } finally {
    if (runs > 1) console.log(`cargo proxy call ms: ${elapsed.join(", ")}`);
  }
});

// Regression justification: catches an unintentional change to the current behavior
// where an ordinary nonzero can retain the 100 ms zero-active-process proof and
// release the lease. This open defect-chain behavior may deliberately change with
// a future contract remediation, which should update this test.
// origin: check-lanes.node-test.mjs:815 "Windows Cargo nonzero waits for descendant zero and releases its lease" -> check-lanes.native-windows.node-test.mjs
test("Windows Cargo nonzero waits for descendant zero and releases its lease", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-failure-"));
  const target = path.join(root, "target");
  const command = {
    label: "natural-nonzero",
    command: process.execPath,
    args: ["-e", "process.exit(7)"],
    cwd: root,
    timeoutMs: 2_000,
    terminationGraceMs: 200,
    settlementTimeoutMs: 3_000,
    maximumLineBytes: 1_024,
    maximumOutputBytes: 4_096,
  };
  try {
    const descendantPidPath = path.join(root, "nonzero-descendant.pid");
    command.args = [
      "-e",
      `const {spawn}=require("node:child_process");const {writeFileSync}=require("node:fs");const d=spawn(process.execPath,["-e","setTimeout(()=>{},300)"],{stdio:"ignore"});writeFileSync(process.argv[1],String(d.pid));process.exit(7)`,
      descendantPidPath,
    ];
    await assert.rejects(
      runCommandGroups([
        {
          label: "rust-natural-failure",
          concurrency: 1,
          timeoutMs: 8_000,
          cargoTargetDir: target,
          repositoryRoot: root,
          commands: [command],
        },
      ]),
      (error) => error?.code === "command-failed" && error.settled === true,
    );
    const descendantPid = Number.parseInt(
      await readFile(descendantPidPath, "utf8"),
      10,
    );
    assert.equal(await processExists(descendantPid), false);
    await assert.rejects(
      readFile(await cargoLeasePath(target, { repositoryRoot: root })),
      /ENOENT/u,
    );
    const retry = (cargoTargetDir) =>
      runCommandGroups([
        {
          label: "rust-immediate-retry",
          concurrency: 1,
          timeoutMs: 6_000,
          cargoTargetDir,
          repositoryRoot: root,
          commands: [
            {
              ...command,
              label: "immediate-success",
              args: ["-e", "process.exit(0)"],
            },
          ],
        },
      ]);
    await retry(target);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// Regression justification: catches a future change that releases a Windows Cargo
// lease before the real Job reports zero active processes, which would weaken the
// actual accounting-observation guarantee even though it is not object settlement.
// origin: check-lanes.node-test.mjs:924 "Windows Cargo success waits for an owned descendant tree to reach zero" -> check-lanes.native-windows.node-test.mjs
test("Windows Cargo success waits for an owned descendant tree to reach zero", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-job-success-"));
  const descendantPidPath = path.join(root, "descendant.pid");
  const descendantDonePath = path.join(root, "descendant.done");
  let descendantPid;
  const source = `
      const { spawn } = require("node:child_process");
      const { writeFileSync } = require("node:fs");
      const descendant = spawn(process.execPath, ["-e", "require('node:fs').writeFileSync(process.argv[1], 'done')", ${JSON.stringify(descendantDonePath)}], {
        detached: true,
        stdio: "ignore",
        windowsHide: true,
      });
      writeFileSync(process.argv[1], String(descendant.pid));
      process.exit(0);
    `;
  try {
    await runCommandGroups([
      {
        label: "rust-owned-success",
        concurrency: 1,
        timeoutMs: 10_000,
        cargoTargetDir: path.join(root, "target"),
        repositoryRoot: root,
        commands: [
          {
            label: "owned-success",
            command: process.execPath,
            args: ["-e", source, descendantPidPath],
            cwd: root,
            timeoutMs: 8_000,
            terminationGraceMs: 200,
            settlementTimeoutMs: 2_000,
            maximumLineBytes: 1_024,
            maximumOutputBytes: 4_096,
          },
        ],
      },
    ]);
    descendantPid = Number.parseInt(
      await readFile(descendantPidPath, "utf8"),
      10,
    );
    assert.equal(await processExists(descendantPid), false);
    assert.equal(await readFile(descendantDonePath, "utf8"), "done");
    await assert.rejects(
      readFile(
        await cargoLeasePath(path.join(root, "target"), {
          repositoryRoot: root,
        }),
      ),
      /ENOENT/u,
    );
  } finally {
    await cleanProcessFixture(root, descendantPidPath, descendantPid);
  }
});

// origin: check-lanes.node-test.mjs:988 "Windows Cargo Job preserves argv cwd environment and bounded output" -> check-lanes.native-windows.node-test.mjs
test("Windows Cargo Job preserves argv cwd environment and bounded output", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-job-io-"));
  const values = ["", "with space", 'quote"inside', "trailing\\"];
  let output = "";
  try {
    await runCommandGroups(
      [
        {
          label: "rust-owned-io",
          concurrency: 1,
          timeoutMs: 5_000,
          cargoTargetDir: path.join(root, "target"),
          repositoryRoot: root,
          commands: [
            {
              label: "owned-io",
              command: process.execPath,
              args: [
                "-e",
                "process.stdout.write(JSON.stringify({argv:process.argv.slice(1),cwd:process.cwd(),env:process.env.SAGE_JOB_TEST}))",
                ...values,
              ],
              cwd: root,
              env: { SAGE_JOB_TEST: "exact-value" },
              timeoutMs: 3_000,
              terminationGraceMs: 200,
              settlementTimeoutMs: 1_000,
              maximumLineBytes: 2_048,
              maximumOutputBytes: 4_096,
            },
          ],
        },
      ],
      {
        executeWindowsCargo: (command, signal, options) =>
          executeWindowsCargoJobCommand(command, signal, {
            ...options,
            stdout: new Writable({
              write(chunk, _encoding, callback) {
                output += chunk.toString();
                callback();
              },
            }),
            stderr: discardStream(),
          }),
      },
    );
    const receipt = JSON.parse(output.replace(/^\[owned-io\] /u, ""));
    assert.deepEqual(receipt.argv, values);
    assert.equal(receipt.cwd, root);
    assert.equal(receipt.env, "exact-value");
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// origin: check-lanes.node-test.mjs:1048 "Windows Cargo Job streams before completion and honors sink backpressure" -> check-lanes.native-windows.node-test.mjs
test("Windows Cargo Job streams before completion and honors sink backpressure", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-job-live-"));
  let output = "";
  let firstDelivered;
  const first = new Promise((resolve) => {
    firstDelivered = resolve;
  });
  const sink = new Writable({
    highWaterMark: 1,
    write(chunk, _encoding, callback) {
      setTimeout(() => {
        output += chunk.toString();
        if (output.includes("first\n")) firstDelivered();
        callback();
      }, 75);
    },
  });
  let settled = false;
  try {
    const execution = executeWindowsCargoJobCommand(
      {
        label: "owned-live",
        command: process.execPath,
        args: [
          "-e",
          "process.stdout.write('first\\n');setTimeout(()=>process.stdout.write('second\\n'),750)",
        ],
        cwd: root,
        timeoutMs: 8_000,
        terminationGraceMs: 500,
        settlementTimeoutMs: 2_000,
        maximumLineBytes: 1_024,
        maximumOutputBytes: 4_096,
      },
      new AbortController().signal,
      { stdout: sink, stderr: discardStream() },
    );
    execution.then(
      () => {
        settled = true;
      },
      () => {
        settled = true;
      },
    );
    const firstTimer = setTimeout(
      () => firstDelivered(new Error("live output was not delivered")),
      6_000,
    );
    const delivery = await first;
    clearTimeout(firstTimer);
    if (delivery instanceof Error) throw delivery;
    assert.equal(settled, false);
    await execution;
    assert.match(output, /\[owned-live\] first\n\[owned-live\] second\n/u);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// origin: check-lanes.node-test.mjs:1112 "Windows Cargo Job streams both outputs near their independent caps" -> check-lanes.native-windows.node-test.mjs
test("Windows Cargo Job streams both outputs near their independent caps", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-job-dual-"));
  let stdout = "";
  let stderr = "";
  const capture = (append) =>
    new Writable({
      write(chunk, _encoding, callback) {
        append(chunk.toString());
        callback();
      },
    });
  try {
    await executeWindowsCargoJobCommand(
      {
        label: "owned-dual",
        command: process.execPath,
        args: [
          "-e",
          "process.stdout.write('A'.repeat(60000));process.stderr.write('B'.repeat(60000))",
        ],
        cwd: root,
        timeoutMs: 8_000,
        terminationGraceMs: 500,
        settlementTimeoutMs: 2_000,
        maximumLineBytes: 65_536,
        maximumOutputBytes: 65_536,
      },
      new AbortController().signal,
      {
        stdout: capture((value) => {
          stdout += value;
        }),
        stderr: capture((value) => {
          stderr += value;
        }),
      },
    );
    assert.equal(stdout.match(/A/gu)?.length, 60_000);
    assert.equal(stderr.match(/B/gu)?.length, 60_000);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// origin: check-lanes.node-test.mjs:1160 "Windows Cargo Job emits a truncation marker and observes grace before force" -> check-lanes.native-windows.node-test.mjs
test("Windows Cargo Job emits a truncation marker and observes grace before force", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-job-grace-"));
  const donePath = path.join(root, "done");
  let output = "";
  try {
    await assert.rejects(
      executeWindowsCargoJobCommand(
        {
          label: "owned-grace",
          command: process.execPath,
          args: [
            "-e",
            `const {writeFileSync}=require("node:fs");process.stdout.write("x".repeat(128));setTimeout(()=>writeFileSync(process.argv[1],"done"),250);setTimeout(()=>process.exit(0),350)`,
            donePath,
          ],
          cwd: root,
          timeoutMs: 8_000,
          terminationGraceMs: 1_000,
          settlementTimeoutMs: 2_000,
          maximumLineBytes: 32,
          maximumOutputBytes: 1_024,
        },
        new AbortController().signal,
        {
          stdout: new Writable({
            write(chunk, _encoding, callback) {
              output += chunk.toString();
              callback();
            },
          }),
          stderr: discardStream(),
        },
      ),
      (error) => error?.code === "output-overflow" && error.settled === true,
    );
    assert.equal(await readFile(donePath, "utf8"), "done");
    assert.equal(output.match(/x/gu)?.length, 32);
    assert.match(output, /\u2026\[truncated\]\n/u);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// origin: check-lanes.node-test.mjs:1207 "Windows Cargo Job timeout proves descendant zero" -> check-lanes.native-windows.node-test.mjs
test("Windows Cargo Job timeout proves descendant zero", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-job-timeout-"));
  const pidPath = path.join(root, "descendant.pid");
  const readyPath = path.join(root, "descendant.ready");
  const target = path.join(root, "target");
  // A non-ASCII image name proves the stall report survives the proof's
  // serialization byte for byte.
  const descendantImage = path.join(root, "caf\u00e9.exe");
  const source = `const {spawn}=require("node:child_process");const fs=require("node:fs");const pidPath=process.argv[1];const readyPath=process.argv[2];const child=spawn(process.argv[3],["-e","require('node:fs').writeFileSync(process.argv[1],'ready');setInterval(()=>{},1000)",readyPath],{detached:true,stdio:"ignore",windowsHide:true});fs.writeFileSync(pidPath,String(child.pid));const deadline=Date.now()+2000;(function waitForReady(){if(fs.existsSync(readyPath))process.exit(0);if(Date.now()>=deadline)throw new Error("descendant-ready handshake timed out");setTimeout(waitForReady,10)})();`;
  let descendantPid;
  try {
    await copyFile(process.execPath, descendantImage);
    await assert.rejects(
      runCommandGroups([
        {
          label: "rust-owned-timeout",
          concurrency: 1,
          timeoutMs: 8_000,
          cargoTargetDir: target,
          repositoryRoot: root,
          commands: [
            {
              label: "owned-timeout",
              command: process.execPath,
              args: ["-e", source, pidPath, readyPath, descendantImage],
              cwd: root,
              // The root has a bounded two-second readiness handshake before
              // its natural exit; preserve a margin so the command timeout
              // observes the intended root-exited surviving-child state.
              timeoutMs: 3_000,
              terminationGraceMs: 500,
              settlementTimeoutMs: 3_000,
              maximumLineBytes: 1_024,
              maximumOutputBytes: 4_096,
            },
          ],
        },
      ]),
      (error) => {
        assert.equal(error?.code, "command-timeout");
        assert.equal(error.settled, true);
        assert.equal(error.exitCode, 0);
        assert.equal(error.teardown, null);
        assert.deepEqual(error.stall, {
          rootRunning: false,
          memberImages: ["caf\u00e9.exe"],
          omittedMembers: 0,
        });
        return true;
      },
    );
    assert.equal(await waitForFixtureReady(readyPath), "ready");
    descendantPid = Number.parseInt(await readFile(pidPath, "utf8"), 10);
    assert.equal(await processExists(descendantPid), false);
    await assert.rejects(
      readFile(await cargoLeasePath(target, { repositoryRoot: root })),
      /ENOENT/u,
    );
    const lease = await acquireCargoTargetLease(target, {
      repositoryRoot: root,
    });
    await lease.release();
  } finally {
    await cleanProcessFixture(root, pidPath, descendantPid);
  }
});

// origin: check-lanes.node-test.mjs:1258 "Windows Cargo Job abort and output overflow prove tree zero" -> check-lanes.native-windows.node-test.mjs
test("Windows Cargo Job abort and output overflow prove tree zero", async () => {
  for (const scenario of ["abort", "overflow"]) {
    const root = await mkdtemp(
      path.join(tmpdir(), `sage-check-job-${scenario}-`),
    );
    const pidPath = path.join(root, "descendant.pid");
    const readyPath = path.join(root, "descendant.ready");
    const source = `const {spawn}=require("node:child_process");const fs=require("node:fs");const pidPath=process.argv[1];const readyPath=process.argv[2];const child=spawn(process.execPath,["-e","require('node:fs').writeFileSync(process.argv[1],'ready');setInterval(()=>{},1000)",readyPath],{detached:true,stdio:"ignore",windowsHide:true});fs.writeFileSync(pidPath,String(child.pid));const deadline=Date.now()+2000;(function waitForReady(){if(fs.existsSync(readyPath)){${
      scenario === "overflow"
        ? "setInterval(()=>process.stdout.write('x'.repeat(2048)),20);return"
        : "setInterval(()=>{},1000);return"
    }}if(Date.now()>=deadline)throw new Error('descendant-ready handshake timed out');setTimeout(waitForReady,10)})();`;
    const controller = new AbortController();
    let descendantPid;
    let executionOutcome;
    try {
      executionOutcome = executeWindowsCargoJobCommand(
        {
          label: `owned-${scenario}`,
          command: process.execPath,
          args: ["-e", source, pidPath, readyPath],
          cwd: root,
          timeoutMs: 5_000,
          terminationGraceMs: 200,
          settlementTimeoutMs: 1_000,
          maximumLineBytes: scenario === "overflow" ? 1_024 : 4_096,
          maximumOutputBytes: 4_096,
        },
        controller.signal,
        {
          repositoryRoot: root,
          stdout: discardStream(),
          stderr: discardStream(),
        },
      ).then(
        (value) => ({ status: "fulfilled", value }),
        (error) => ({ status: "rejected", error }),
      );
      assert.equal(await waitForFixtureReady(readyPath), "ready");
      if (scenario === "abort") controller.abort(new Error("test abort"));
      const outcome = await executionOutcome;
      assert.equal(outcome.status, "rejected");
      assert.equal(
        outcome.error?.code,
        scenario === "abort" ? "command-aborted" : "output-overflow",
      );
      assert.equal(outcome.error?.settled, true);
      descendantPid = Number.parseInt(await readFile(pidPath, "utf8"), 10);
      assert.equal(await processExists(descendantPid), false);
    } finally {
      controller.abort(new Error("fixture cleanup"));
      await settleFixtureExecution(executionOutcome, `owned-${scenario}`);
      await cleanProcessFixture(root, pidPath, descendantPid);
    }
  }
});

// origin: check-lanes.node-test.mjs:662 "case variants contend across target materialization on case-insensitive hosts" -> check-lanes.native-windows.node-test.mjs (Windows arm, win32 lease-identity folding on NTFS)
// Host capability: requires native Windows with a case-insensitive (NTFS)
// host path. Off-host the contention assertion fails closed, never passes.
test("case variants contend across target materialization on case-insensitive hosts [native-Windows/NTFS]", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-check-case-windows-"));
  let lease;
  try {
    lease = await acquireCargoTargetLease(path.join(root, "Target", "New"), {
      repositoryRoot: root,
      platform: "win32",
    });
    await assert.rejects(
      acquireCargoTargetLease(path.join(root, "target", "new"), {
        repositoryRoot: root,
        platform: "win32",
      }),
      (error) => error?.code === "cargo-target-locked",
    );
  } finally {
    await lease?.release();
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

async function waitForFixtureReady(pathname, timeoutMs = 2_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const receipt = await readFile(pathname, "utf8");
      if (receipt === "ready") return receipt;
      throw new Error(`fixture ready receipt was malformed at ${pathname}`);
    } catch (error) {
      if (error?.code !== "ENOENT") throw error;
    }
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  assert.fail(`fixture ready receipt was unavailable at ${pathname}`);
}

async function settleFixtureExecution(outcome, label, timeoutMs = 5_000) {
  if (!outcome) return;
  let timer;
  const timedOut = new Promise((resolve) => {
    timer = setTimeout(() => resolve(undefined), timeoutMs);
  });
  try {
    const settled = await Promise.race([outcome, timedOut]);
    if (settled === undefined) {
      assert.fail(`${label} did not settle before fixture cleanup`);
    }
    return settled;
  } finally {
    clearTimeout(timer);
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
