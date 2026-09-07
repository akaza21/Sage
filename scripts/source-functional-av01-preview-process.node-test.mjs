import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { decodeAv01FixtureProcessHelperOutput } from "./source-functional-av01-preview-probe.mjs";

const powershell = path.join(
  process.env.SystemRoot ?? "C:\\Windows",
  "System32",
  "WindowsPowerShell",
  "v1.0",
  "powershell.exe",
);
const helper = path.resolve(
  "scripts",
  "source-functional-av01-preview-process.ps1",
);
const nonce = "e".repeat(64);

function waitForFile(filePath, deadline) {
  return new Promise((resolve, reject) => {
    const poll = () => {
      if (existsSync(filePath)) return resolve();
      if (Date.now() >= deadline)
        return reject(
          new Error(`timed out waiting for ${path.basename(filePath)}`),
        );
      setTimeout(poll, 25);
    };
    poll();
  });
}

function writeSignal(filePath) {
  if (!existsSync(filePath))
    writeFileSync(filePath, "signal\n", { flag: "wx" });
}

function waitForExit(child, deadline) {
  if (child.exitCode !== null) return Promise.resolve(child.exitCode);
  return new Promise((resolve, reject) => {
    const timer = setTimeout(
      () => reject(new Error("bootstrap-cleanup-timeout")),
      Math.max(1, deadline - Date.now()),
    );
    child.once("exit", (code) => {
      clearTimeout(timer);
      resolve(code);
    });
    child.once("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
  });
}

async function cleanBootstrap(
  parent,
  { dispose, disposed, cleanup },
  deadline,
) {
  let disposalError = null;
  if (parent.exitCode === null) {
    writeSignal(dispose);
    try {
      await waitForFile(disposed, deadline);
    } catch (error) {
      disposalError = error;
    } finally {
      writeSignal(cleanup);
    }
  }
  const exitCode = await waitForExit(
    parent,
    Math.max(deadline, Date.now() + 10_000),
  );
  if (exitCode !== 0) throw new Error(`bootstrap-exit-${exitCode}`);
  return { disposalError };
}

function waitForJsonLine(child, deadline) {
  return new Promise((resolve, reject) => {
    let output = "";
    let errors = "";
    const timer = setTimeout(
      () => reject(new Error("bootstrap-timeout")),
      deadline - Date.now(),
    );
    child.stdout.on("data", (chunk) => {
      output += chunk;
      const line = output.split(/\r?\n/u).find(Boolean);
      if (!line) return;
      clearTimeout(timer);
      try {
        resolve(JSON.parse(line));
      } catch (error) {
        reject(error);
      }
    });
    child.stderr.on("data", (chunk) => (errors += chunk));
    child.once("error", reject);
    child.once("exit", (code) => {
      if (!output) {
        clearTimeout(timer);
        reject(new Error(errors || `bootstrap-exited-${code}`));
      }
    });
  });
}

function invokeHelper(argumentsFor) {
  return spawnSync(
    powershell,
    ["-NoProfile", "-NonInteractive", "-File", helper, ...argumentsFor],
    {
      encoding: "utf8",
      windowsHide: true,
      timeout: 10_000,
    },
  );
}

function helperArguments(root, startup, parent, mode, identity) {
  const values = [
    "-Mode",
    mode,
    "-FixtureRoot",
    root,
    "-StartupPath",
    startup,
    "-Nonce",
    nonce,
    "-SageProcessId",
    String(parent.processId),
    "-SageCreationDate",
    parent.creationDate,
    "-SageExecutablePath",
    parent.executablePath,
  ];
  if (identity)
    values.push(
      "-IdentityJson",
      Buffer.from(JSON.stringify(identity)).toString("base64"),
    );
  return values;
}

test("AV01 process helper captures only an owned exact child and proves its disappearance before parent cleanup", async () => {
  assert.equal(
    process.platform,
    "win32",
    "native-Windows budget lane required",
  );
  const root = mkdtempSync(path.join(tmpdir(), "sage-av01-process-helper-"));
  const startup = path.join(root, "av01-preview-startup.json");
  const dispose = path.join(root, "dispose");
  const disposed = path.join(root, "disposed");
  const cleanup = path.join(root, "cleanup");
  const bootstrap = path.join(root, "bootstrap.ps1");
  let parent;
  let cleanupComplete = false;
  let cleanupDisposalError;
  const deadline = Date.now() + 75_000;
  try {
    writeFileSync(
      bootstrap,
      `param([string]$Dispose,[string]$Disposed,[string]$Cleanup,[string]$NodePath)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$child=$null
function Wait-Signal([string]$Path,[string]$Label) {
  $clock=[Diagnostics.Stopwatch]::StartNew()
  while(-not(Test-Path -LiteralPath $Path)){
    if($clock.ElapsedMilliseconds -ge 30000){throw "timed out waiting for $Label"}
    Start-Sleep -Milliseconds 25
  }
}
try {
  $child=Start-Process -FilePath $NodePath -ArgumentList @('-e','setTimeout(()=>{},120000)') -PassThru -WindowStyle Hidden
  $self=Get-Process -Id $PID
  [ordered]@{processId=$PID;creationDate=$self.StartTime.ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fff'Z'");executablePath=$self.Path;childProcessId=$child.Id}|ConvertTo-Json -Compress
  Wait-Signal $Dispose 'dispose'
  $child.Refresh()
  if(-not $child.HasExited){$child.Kill();if(-not $child.WaitForExit(5000)){throw 'owned child did not exit'}}
  Set-Content -LiteralPath $Disposed -Value 'disposed' -NoNewline
  Wait-Signal $Cleanup 'cleanup'
} finally {
  if($null -ne $child){try{$child.Refresh();if(-not $child.HasExited){$child.Kill();if(-not $child.WaitForExit(5000)){throw 'owned child did not exit'}}}catch{exit 1}}
}`,
    );
    parent = spawn(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-File",
        bootstrap,
        "-Dispose",
        dispose,
        "-Disposed",
        disposed,
        "-Cleanup",
        cleanup,
        "-NodePath",
        process.execPath,
      ],
      { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] },
    );
    assert.ok(path.isAbsolute(process.execPath));
    const parentIdentity = await waitForJsonLine(parent, deadline);
    writeFileSync(
      startup,
      `${JSON.stringify({
        schemaVersion: 1,
        processId: parentIdentity.childProcessId,
        nonce,
        case: "starting-active",
        startedAt: new Date().toISOString(),
      })}\n`,
      { flag: "wx" },
    );

    const capture = invokeHelper(
      helperArguments(root, startup, parentIdentity, "capture"),
    );
    assert.equal(capture.status, 0, `${capture.stdout}\n${capture.stderr}`);
    const identity = JSON.parse(capture.stdout.trim());
    assert.equal(identity.nonce, nonce);
    assert.equal(identity.processId, parentIdentity.childProcessId);
    assert.equal(identity.sageProcessId, parentIdentity.processId);
    assert.throws(
      () =>
        decodeAv01FixtureProcessHelperOutput(capture.stdout, "capture", {
          nonce: `${nonce.slice(0, -1)}f`,
          sageProcessId: parentIdentity.processId,
        }),
      /av01-preview-fixture-process-json/,
    );

    const live = invokeHelper(
      helperArguments(root, startup, parentIdentity, "absent", identity),
    );
    assert.notEqual(live.status, 0);
    assert.match(`${live.stdout}\n${live.stderr}`, /fixture-still-live/);
    const wrongSage = invokeHelper(
      helperArguments(
        root,
        startup,
        { ...parentIdentity, creationDate: "2026-01-01T00:00:00.000Z" },
        "capture",
      ),
    );
    assert.notEqual(wrongSage.status, 0);
    assert.match(`${wrongSage.stdout}\n${wrongSage.stderr}`, /sage-identity/);
    const malformed = invokeHelper([
      ...helperArguments(root, startup, parentIdentity, "absent"),
      "-IdentityJson",
      "not-base64",
    ]);
    assert.notEqual(malformed.status, 0);
    assert.match(`${malformed.stdout}\n${malformed.stderr}`, /identity-json/);

    writeSignal(dispose);
    await waitForFile(disposed, deadline);
    const absent = invokeHelper(
      helperArguments(root, startup, parentIdentity, "absent", identity),
    );
    assert.equal(absent.status, 0, `${absent.stdout}\n${absent.stderr}`);
    const absence = JSON.parse(absent.stdout.trim());
    assert.equal(absence.absent, true);
    assert.equal(absence.processId, identity.processId);
    assert.throws(
      () =>
        decodeAv01FixtureProcessHelperOutput(
          JSON.stringify({
            ...absence,
            creationDate: "2026-09-07T12:00:01.000Z",
          }),
          "absent",
          identity,
        ),
      /av01-preview-fixture-process-json/,
    );
    writeSignal(cleanup);
    assert.equal(await waitForExit(parent, deadline), 0);
    cleanupComplete = true;
  } finally {
    let cleanupResult = null;
    if (parent && !cleanupComplete) {
      try {
        cleanupResult = await cleanBootstrap(
          parent,
          { dispose, disposed, cleanup },
          deadline,
        );
        cleanupComplete = true;
      } finally {
        if (cleanupComplete) rmSync(root, { recursive: true, force: true });
      }
    }
    cleanupDisposalError = cleanupResult?.disposalError ?? null;
    if (!parent || cleanupComplete)
      rmSync(root, { recursive: true, force: true });
  }
  if (cleanupDisposalError) throw cleanupDisposalError;
});
