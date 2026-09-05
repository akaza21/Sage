import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

function readUtf8Receipt(file) {
  const text = readFileSync(file, "utf8");
  return text.charCodeAt(0) === 0xfeff ? text.slice(1) : text;
}

function strictPidReceipt(file, label) {
  let text;
  try {
    text = readUtf8Receipt(file).trim();
  } catch (error) {
    if (error?.code === "ENOENT") return undefined;
    throw new Error(
      `cannot read fixture receipt ${file} for ${label}: ${error.message}`,
      { cause: error },
    );
  }
  if (!/^\d+$/.test(text))
    throw new Error(`malformed fixture receipt ${file} for ${label}`);
  const pid = Number(text);
  if (!Number.isSafeInteger(pid) || pid < 1)
    throw new Error(`invalid fixture PID receipt ${file} for ${label}`);
  return pid;
}

function strictIdentity(file, label, root, requiredCommandParts) {
  let value;
  try {
    value = JSON.parse(readUtf8Receipt(file));
  } catch (error) {
    throw new Error(
      `malformed fixture identity ${file} for ${label}: ${error.message}`,
      { cause: error },
    );
  }
  if (
    !Number.isSafeInteger(value?.pid) ||
    value.pid < 1 ||
    typeof value?.creationFileTime !== "string" ||
    !/^\d+$/.test(value.creationFileTime) ||
    value.creationFileTime === "0" ||
    String(value?.executablePath ?? "").toLowerCase() !==
      process.execPath.toLowerCase() ||
    typeof value?.commandLine !== "string" ||
    !value.commandLine.includes(root) ||
    requiredCommandParts.some((part) => !value.commandLine.includes(part))
  ) {
    throw new Error(`invalid exact fixture identity for ${label}`);
  }
  return value;
}

test("fixture identity receipts accept normal and Windows PowerShell UTF-8 BOM JSON", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-windows-job-receipt-"));
  const helper = path.join(root, "fixture-helper.mjs");
  const plain = path.join(root, "plain.identity.json");
  const bom = path.join(root, "bom.identity.json");
  const malformed = path.join(root, "malformed.identity.json");
  const identity = {
    pid: 1234,
    creationFileTime: "134330804914510753",
    executablePath: process.execPath,
    commandLine: `"${process.execPath}" "${helper}" "${root}"`,
  };
  try {
    writeFileSync(plain, JSON.stringify(identity), "utf8");
    writeFileSync(bom, `\ufeff${JSON.stringify(identity)}`, "utf8");
    writeFileSync(malformed, "\ufeff{", "utf8");
    assert.deepEqual(
      strictIdentity(plain, "plain receipt", root, [helper]),
      identity,
    );
    assert.deepEqual(
      strictIdentity(bom, "BOM receipt", root, [helper]),
      identity,
    );
    assert.throws(() =>
      strictIdentity(malformed, "malformed receipt", root, [helper]),
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

const handleCleanupSource = String.raw`
using System; using System.ComponentModel; using System.Runtime.InteropServices; using System.Text;
public static class SageFixtureHandle {
  const uint QUERY=0x1000, TERMINATE=1, SYNCHRONIZE=0x100000, WAIT_OBJECT_0=0, WAIT_TIMEOUT=258;
  [StructLayout(LayoutKind.Sequential)] struct U { public ushort Length, MaximumLength; public IntPtr Buffer; }
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint access,bool inherit,int pid);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll", SetLastError=true)] static extern int GetProcessId(IntPtr h);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetProcessTimes(IntPtr h,out long c,out long e,out long k,out long u);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool QueryFullProcessImageName(IntPtr h,uint f,StringBuilder b,ref int n);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool TerminateProcess(IntPtr h,uint code);
  [DllImport("kernel32.dll", SetLastError=true)] static extern uint WaitForSingleObject(IntPtr h,uint ms);
  [DllImport("ntdll.dll")] static extern int NtQueryInformationProcess(IntPtr h,int c,IntPtr b,int n,out int r);
  static void Check(bool ok,string op) { if(!ok) throw new Win32Exception(Marshal.GetLastWin32Error(),op+" failed"); }
  static string Command(IntPtr h) { int n; NtQueryInformationProcess(h,60,IntPtr.Zero,0,out n); if(n<=0) throw new InvalidOperationException("ProcessCommandLineInformation did not size a buffer."); IntPtr b=Marshal.AllocHGlobal(n); try { if(NtQueryInformationProcess(h,60,b,n,out n)!=0) throw new InvalidOperationException("ProcessCommandLineInformation failed."); var u=(U)Marshal.PtrToStructure(b,typeof(U)); if(u.Buffer==IntPtr.Zero || u.Length==0) throw new InvalidOperationException("ProcessCommandLineInformation returned no command."); return Marshal.PtrToStringUni(u.Buffer,u.Length/2); } finally { Marshal.FreeHGlobal(b); } }
  static void Verify(IntPtr h,int pid,long creation,string exe,string command) { if(GetProcessId(h)!=pid) throw new InvalidOperationException("fixture PID changed under retained handle."); long c,e,k,u; Check(GetProcessTimes(h,out c,out e,out k,out u),"GetProcessTimes"); if(c!=creation) throw new InvalidOperationException("fixture creation identity changed."); var b=new StringBuilder(32768); int n=b.Capacity; Check(QueryFullProcessImageName(h,0,b,ref n),"QueryFullProcessImageName"); if(!String.Equals(b.ToString(),exe,StringComparison.OrdinalIgnoreCase)) throw new InvalidOperationException("fixture executable identity changed."); if(!String.Equals(Command(h),command,StringComparison.Ordinal)) throw new InvalidOperationException("fixture command identity changed."); }
  public static string Cleanup(int pid,long creation,string exe,string command) { IntPtr h=OpenProcess(QUERY|TERMINATE|SYNCHRONIZE,false,pid); if(h==IntPtr.Zero) { int e=Marshal.GetLastWin32Error(); if(e==87) return "absent"; throw new Win32Exception(e,"OpenProcess failed"); } try { Verify(h,pid,creation,exe,command); Verify(h,pid,creation,exe,command); Check(TerminateProcess(h,1),"TerminateProcess"); uint w=WaitForSingleObject(h,2000); if(w!=WAIT_OBJECT_0) throw new InvalidOperationException(w==WAIT_TIMEOUT?"fixture handle wait timed out.":"fixture handle wait failed."); return "reaped"; } finally { Check(CloseHandle(h),"CloseHandle"); } }
}`;

function cleanExactSpec(root, spec) {
  const receipts = spec.pidFiles.map((file) =>
    strictPidReceipt(file, spec.label),
  );
  const present = receipts.filter((pid) => pid !== undefined);
  if (present.length === 0) {
    try {
      readFileSync(spec.identityFile, "utf8");
    } catch (error) {
      if (error?.code === "ENOENT") return;
      throw new Error(
        `cannot read fixture identity ${spec.identityFile} for ${spec.label}: ${error.message}`,
        { cause: error },
      );
    }
    throw new Error(
      `fixture identity exists without a PID receipt for ${spec.label}`,
    );
  }
  if (new Set(present).size !== 1)
    throw new Error(`ambiguous fixture PIDs for ${spec.label}`);
  const identity = strictIdentity(
    spec.identityFile,
    spec.label,
    root,
    spec.commandParts ?? [],
  );
  const pid = present[0];
  if (identity.pid !== pid)
    throw new Error(
      `fixture PID receipt disagrees with identity for ${spec.label}`,
    );
  const cleanup = path.join(
    root,
    `cleanup-${spec.label.replace(/[^a-z0-9]+/gi, "-")}.ps1`,
  );
  writeFileSync(
    cleanup,
    `param($PidValue,$IdentityJson)\nAdd-Type -TypeDefinition @'\n${handleCleanupSource}\n'@\n$id=$IdentityJson | ConvertFrom-Json\n[SageFixtureHandle]::Cleanup([int]$PidValue,[long]$id.creationFileTime,[string]$id.executablePath,[string]$id.commandLine)`,
  );
  const outcome = execFileSync(
    "powershell.exe",
    [
      "-NoLogo",
      "-NoProfile",
      "-NonInteractive",
      "-File",
      cleanup,
      String(pid),
      JSON.stringify(identity),
    ],
    { encoding: "utf8", timeout: 5000, windowsHide: true },
  ).trim();
  if (outcome !== "absent" && outcome !== "reaped")
    throw new Error(
      `unexpected fixture cleanup receipt for ${spec.label}: ${outcome}`,
    );
}

function cleanRoots(root, specs) {
  const errors = [];
  for (const spec of specs) {
    try {
      cleanExactSpec(root, spec);
    } catch (error) {
      errors.push(error);
    }
  }
  if (errors.length)
    throw new AggregateError(
      errors,
      `exact fixture cleanup failed for ${root}`,
    );
  rmSync(root, { recursive: true, force: true });
}

test("native Job launch preserves exact argv and proves active-process convergence", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-windows-job-"));
  try {
    const values = [
      "",
      "plain",
      "with space",
      'quote"inside',
      "trailing\\",
      'slashes\\\\"quote',
      "carriage\rreturn",
      "form\ffeed",
      "unicode-λ",
    ];
    const helper = path.join(root, "argv-helper.mjs");
    const result = path.join(root, "argv.json");
    writeFileSync(
      helper,
      `import fs from "node:fs";\nconst [output, ...values] = process.argv.slice(2);\nsetTimeout(() => fs.writeFileSync(output, JSON.stringify(values)), 250);\n`,
    );
    const modulePath = path.resolve("scripts/footprint/WindowsJob.psm1");
    const completionModulePath = path.resolve(
      "scripts/footprint/WindowsJobCompletion.psm1",
    );
    const driver = path.join(root, "driver.ps1");
    const encodedValues = Buffer.from(JSON.stringify(values)).toString(
      "base64",
    );
    writeFileSync(
      driver,
      `param([string]$ModulePath,[string]$CompletionModulePath,[string]$NodePath,[string]$Helper,[string]$Result,[string]$EncodedValues)\n[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)\nImport-Module $ModulePath -Force\nImport-Module $CompletionModulePath -Force\n[string[]]$values = ([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($EncodedValues)) | ConvertFrom-Json)\n$info = [Diagnostics.ProcessStartInfo]::new()\n$info.FileName = $NodePath\n$info.WorkingDirectory = Split-Path -Parent $Helper\n$info.UseShellExecute = $false\n$info.CreateNoWindow = $true\n$launch = Start-FootprintJobProcess $info -ArgumentList (@($Helper,$Result) + $values) -ActiveProcessLimit 4\ntry { $active = [uint32]$launch.boundary.ActiveProcesses()\n$cleanup = Wait-FootprintJobProcess $launch.process $launch.boundary 5000 200\n[string[]]$argv = ([IO.File]::ReadAllText($Result,[Text.Encoding]::UTF8) | ConvertFrom-Json)\n[ordered]@{ active = $active; orphan = $cleanup.orphanProcessesAfterClose; argv = $argv } | ConvertTo-Json -Compress } finally { try { $launch.boundary.Dispose() } catch {} ; try { if ($null -ne $launch.process) { $launch.process.Dispose() } } catch {} }\n`,
    );
    const stdout = execFileSync(
      "powershell.exe",
      [
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        driver,
        modulePath,
        completionModulePath,
        process.execPath,
        helper,
        result,
        encodedValues,
      ],
      { encoding: "utf8", timeout: 20_000, windowsHide: true },
    );
    const observed = JSON.parse(stdout.trim());
    assert.ok(observed.active >= 1);
    assert.equal(observed.orphan, 0);
    assert.deepEqual(observed.argv, values);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("native Job reports intentional root exit 0 after stable Job zero", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-windows-job-"));
  const pidFile = path.join(root, "root.pid");
  const identityFile = path.join(root, "root.identity.json");
  let helper;
  try {
    helper = path.join(root, "exit-zero.mjs");
    const marker = path.join(root, "ready.marker");
    writeFileSync(
      helper,
      `import fs from "node:fs";\nfs.writeFileSync(process.argv[3], String(process.pid));\nfs.writeFileSync(process.argv[2], "ready");\nprocess.exit(0);\n`,
    );
    const modulePath = path.resolve("scripts/footprint/WindowsJob.psm1");
    const completionModulePath = path.resolve(
      "scripts/footprint/WindowsJobCompletion.psm1",
    );
    const driver = path.join(root, "driver-exit-zero.ps1");
    writeFileSync(
      driver,
      [
        `param([string]$ModulePath,[string]$CompletionModulePath,[string]$NodePath,[string]$Helper,[string]$Marker,[string]$PidFile,[string]$IdentityFile,[string]$FixtureRoot)`,
        `[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)`,
        `Import-Module $ModulePath -Force`,
        `Import-Module $CompletionModulePath -Force`,
        `$info = [Diagnostics.ProcessStartInfo]::new()`,
        `$info.FileName = $NodePath`,
        `$info.WorkingDirectory = Split-Path -Parent $Helper`,
        `$info.UseShellExecute = $false`,
        `$info.CreateNoWindow = $true`,
        `$launch = Start-FootprintJobProcess $info -ArgumentList @($Helper,$Marker,$PidFile) -ActiveProcessLimit 4`,
        `[ordered]@{ pid=[int]$launch.boundary.ProcessId; executablePath=$launch.boundary.RootImagePath(); commandLine=$launch.boundary.RootCommandLine; creationFileTime=$launch.boundary.RootCreationFileTime().ToString() } | ConvertTo-Json -Compress | Out-File -LiteralPath $IdentityFile -Encoding utf8`,
        `try {`,
        `  $cleanup = Wait-FootprintJobProcess $launch.process $launch.boundary 5000 200`,
        `  if ($cleanup.exitCode -ne 0) { throw ('expected exit 0 got ' + $cleanup.exitCode) }`,
        `  if ($cleanup.orphanProcessesAfterClose -ne 0) { throw 'orphan not zero' }`,
        `  if (-not $cleanup.treeBoundaryVerified) { throw 'tree boundary not verified' }`,
        `  if (-not (Test-Path -LiteralPath $Marker)) { throw 'marker missing' }`,
        `  [ordered]@{ exitCode = $cleanup.exitCode; orphan = $cleanup.orphanProcessesAfterClose; verified = $cleanup.treeBoundaryVerified } | ConvertTo-Json -Compress`,
        `} finally { try { $launch.boundary.Dispose() } catch {} ; try { if ($null -ne $launch.process) { $launch.process.Dispose() } } catch {} }`,
      ].join("\n"),
    );
    const stdout = execFileSync(
      "powershell.exe",
      [
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        driver,
        modulePath,
        completionModulePath,
        process.execPath,
        helper,
        marker,
        pidFile,
        identityFile,
        root,
      ],
      { encoding: "utf8", timeout: 20_000, windowsHide: true },
    );
    const observed = JSON.parse(stdout.trim());
    assert.equal(observed.exitCode, 0);
    assert.equal(observed.orphan, 0);
    assert.equal(observed.verified, true);
  } finally {
    cleanRoots(root, [
      {
        pidFiles: [pidFile],
        identityFile,
        label: "exit-0 root",
        commandParts: [helper],
      },
    ]);
  }
});

test("native Job reports intentional root nonzero including immediate exit", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-windows-job-"));
  const delayedPid = path.join(root, "delayed.pid");
  const delayedIdentity = path.join(root, "delayed.identity.json");
  const immediatePid = path.join(root, "immediate.pid");
  const immediateIdentity = path.join(root, "immediate.identity.json");
  let delayedHelper;
  let immediateHelper;
  try {
    delayedHelper = path.join(root, "exit-delayed.mjs");
    immediateHelper = path.join(root, "exit-immediate.mjs");
    const delayedMarker = path.join(root, "delayed.marker");
    writeFileSync(
      delayedHelper,
      `import fs from "node:fs";\nfs.writeFileSync(process.argv[3], String(process.pid));\nfs.writeFileSync(process.argv[2], "ready");\nsetTimeout(() => process.exit(7), 200);\n`,
    );
    writeFileSync(
      immediateHelper,
      `import fs from "node:fs";\ntry { fs.writeFileSync(process.argv[2], String(process.pid)); } catch {} \nprocess.exit(7);\n`,
    );
    const modulePath = path.resolve("scripts/footprint/WindowsJob.psm1");
    const completionModulePath = path.resolve(
      "scripts/footprint/WindowsJobCompletion.psm1",
    );
    const driver = path.join(root, "driver-nonzero.ps1");
    writeFileSync(
      driver,
      [
        `param([string]$ModulePath,[string]$CompletionModulePath,[string]$NodePath,[string]$DelayedHelper,[string]$DelayedMarker,[string]$DelayedPid,[string]$DelayedIdentity,[string]$ImmediateHelper,[string]$ImmediatePid,[string]$ImmediateIdentity)`,
        `[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)`,
        `Import-Module $ModulePath -Force`,
        `Import-Module $CompletionModulePath -Force`,
        `function Start-And-Wait([string]$Helper,[string[]]$ExtraArgs,[string]$IdentityFile) {`,
        `  $info = [Diagnostics.ProcessStartInfo]::new()`,
        `  $info.FileName = $NodePath`,
        `  $info.WorkingDirectory = Split-Path -Parent $Helper`,
        `  $info.UseShellExecute = $false`,
        `  $info.CreateNoWindow = $true`,
        `  $launch = Start-FootprintJobProcess $info -ArgumentList (@($Helper) + $ExtraArgs) -ActiveProcessLimit 4`,
        `  [ordered]@{ pid=[int]$launch.boundary.ProcessId; executablePath=$launch.boundary.RootImagePath(); commandLine=$launch.boundary.RootCommandLine; creationFileTime=$launch.boundary.RootCreationFileTime().ToString() } | ConvertTo-Json -Compress | Out-File -LiteralPath $IdentityFile -Encoding utf8`,
        `  try {`,
        `    $cleanup = Wait-FootprintJobProcess $launch.process $launch.boundary 5000 200`,
        `    return $cleanup`,
        `  } finally { try { $launch.boundary.Dispose() } catch {} ; try { if ($null -ne $launch.process) { $launch.process.Dispose() } } catch {} }`,
        `}`,
        `$delayed = Start-And-Wait $DelayedHelper @($DelayedMarker,$DelayedPid) $DelayedIdentity`,
        `if ($delayed.exitCode -ne 7) { throw ('delayed expected 7 got ' + $delayed.exitCode) }`,
        `if ($delayed.orphanProcessesAfterClose -ne 0) { throw 'delayed orphan not zero' }`,
        `$immediate = Start-And-Wait $ImmediateHelper @($ImmediatePid) $ImmediateIdentity`,
        `if ($immediate.exitCode -ne 7) { throw ('immediate expected 7 got ' + $immediate.exitCode) }`,
        `if ($immediate.orphanProcessesAfterClose -ne 0) { throw 'immediate orphan not zero' }`,
        `if (-not (Test-Path -LiteralPath $DelayedMarker)) { throw 'delayed marker missing' }`,
        `[ordered]@{ delayed = $delayed.exitCode; immediate = $immediate.exitCode; orphanDelayed = $delayed.orphanProcessesAfterClose; orphanImmediate = $immediate.orphanProcessesAfterClose } | ConvertTo-Json -Compress`,
      ].join("\n"),
    );
    const stdout = execFileSync(
      "powershell.exe",
      [
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        driver,
        modulePath,
        completionModulePath,
        process.execPath,
        delayedHelper,
        delayedMarker,
        delayedPid,
        delayedIdentity,
        immediateHelper,
        immediatePid,
        immediateIdentity,
      ],
      { encoding: "utf8", timeout: 20_000, windowsHide: true },
    );
    const observed = JSON.parse(stdout.trim());
    assert.equal(observed.delayed, 7);
    assert.equal(observed.immediate, 7);
    assert.equal(observed.orphanDelayed, 0);
    assert.equal(observed.orphanImmediate, 0);
  } finally {
    cleanRoots(root, [
      {
        pidFiles: [delayedPid],
        identityFile: delayedIdentity,
        label: "delayed-7 root",
        commandParts: [delayedHelper],
      },
      {
        pidFiles: [immediatePid],
        identityFile: immediateIdentity,
        label: "immediate-7 root",
        commandParts: [immediateHelper],
      },
    ]);
  }
});

test("native Job root exit with surviving child never becomes false success", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-windows-job-"));
  const rootPidFile = path.join(root, "root.pid");
  const rootIdentity = path.join(root, "root.identity.json");
  const childPidFile = path.join(root, "child.pid");
  const childSpawned = path.join(root, "child.pid.spawned");
  const childReady = path.join(root, "child.ready");
  const childIdentity = path.join(root, "child.identity.json");
  let childHelper;
  let rootHelper;
  try {
    childHelper = path.join(root, "child-sleeper.mjs");
    rootHelper = path.join(root, "root-with-child.mjs");
    writeFileSync(
      childHelper,
      `import fs from "node:fs";\nfs.writeFileSync(process.argv[2], String(process.pid));\nfs.writeFileSync(process.argv[3], "ready");\nsetTimeout(() => {}, 30000);\n`,
    );
    writeFileSync(
      rootHelper,
      `import { spawn } from "node:child_process";\nimport fs from "node:fs";\nconst childHelper = process.argv[2];\nconst childPidFile = process.argv[3];\nconst rootPidFile = process.argv[4];\nconst childReady = process.argv[5];\nconst child = spawn(process.execPath, [childHelper, childPidFile, childReady], { detached: true, stdio: "ignore", windowsHide: true });\nfs.writeFileSync(childPidFile + ".spawned", String(child.pid));\nconst deadline = Date.now() + 2000;\nwhile (!fs.existsSync(childReady) && Date.now() < deadline) await new Promise((resolve) => setTimeout(resolve, 10));\nif (!fs.existsSync(childReady)) throw new Error("child-ready handshake timed out");\nfs.writeFileSync(rootPidFile, String(process.pid));\nprocess.exit(0);\n`,
    );
    const modulePath = path.resolve("scripts/footprint/WindowsJob.psm1");
    const completionModulePath = path.resolve(
      "scripts/footprint/WindowsJobCompletion.psm1",
    );
    const driver = path.join(root, "driver-child.ps1");
    writeFileSync(
      driver,
      [
        `param([string]$ModulePath,[string]$CompletionModulePath,[string]$NodePath,[string]$RootHelper,[string]$ChildHelper,[string]$ChildPid,[string]$ChildReady,[string]$RootPid,[string]$RootIdentityFile,[string]$ChildIdentityFile)`,
        `[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)`,
        `Import-Module $ModulePath -Force`,
        `Import-Module $CompletionModulePath -Force`,
        `$info = [Diagnostics.ProcessStartInfo]::new()`,
        `$info.FileName = $NodePath`,
        `$info.WorkingDirectory = Split-Path -Parent $RootHelper`,
        `$info.UseShellExecute = $false`,
        `$info.CreateNoWindow = $true`,
        `$launch = Start-FootprintJobProcess $info -ArgumentList @($RootHelper,$ChildHelper,$ChildPid,$RootPid,$ChildReady) -ActiveProcessLimit 4`,
        `[ordered]@{ pid=[int]$launch.boundary.ProcessId; executablePath=$launch.boundary.RootImagePath(); commandLine=$launch.boundary.RootCommandLine; creationFileTime=$launch.boundary.RootCreationFileTime().ToString() } | ConvertTo-Json -Compress | Out-File -LiteralPath $RootIdentityFile -Encoding utf8`,
        `try {`,
        `  $spawnDeadline = [Diagnostics.Stopwatch]::GetTimestamp() + [Diagnostics.Stopwatch]::Frequency * 2`,
        `  while ((-not (Test-Path -LiteralPath ($ChildPid + '.spawned')) -or -not (Test-Path -LiteralPath $ChildReady)) -and [Diagnostics.Stopwatch]::GetTimestamp() -lt $spawnDeadline) { Start-Sleep -Milliseconds 10 }`,
        `  if (-not (Test-Path -LiteralPath ($ChildPid + '.spawned')) -or -not (Test-Path -LiteralPath $ChildReady)) { throw 'bounded child-ready handshake did not complete' }`,
        `  $spawnedPid = [int]([IO.File]::ReadAllText($ChildPid + '.spawned').Trim())`,
        `  $childCap = @(Get-CimInstance Win32_Process -Filter ('ProcessId=' + $spawnedPid) -ErrorAction Stop)`,
        `  if ($childCap.Count -ne 1) { throw 'surviving child identity not found immediately after spawned receipt' }`,
        `  if (@(Get-Process -Id $spawnedPid -ErrorAction SilentlyContinue).Count -ne 1) { throw 'ready descendant was not live before the root wait' }`,
        `  $childProcess = [Diagnostics.Process]::GetProcessById($spawnedPid)`,
        `  try {`,
        `    $null = $childProcess.Handle`,
        `    $childCreationFileTime = $childProcess.StartTime.ToFileTimeUtc()`,
        `    $childPostCap = @(Get-CimInstance Win32_Process -Filter ('ProcessId=' + $spawnedPid) -ErrorAction Stop)`,
        `    if ($childPostCap.Count -ne 1) { throw 'surviving child identity disappeared after retained-handle capture' }`,
        `    if ([int]$childPostCap[0].ProcessId -ne $spawnedPid -or [string]$childPostCap[0].ExecutablePath -ne [string]$childCap[0].ExecutablePath -or [string]$childPostCap[0].CommandLine -ne [string]$childCap[0].CommandLine) { throw 'surviving child identity changed across fresh CIM queries' }`,
        `    if ([string]$childPostCap[0].ExecutablePath -ne $NodePath -or -not [string]$childPostCap[0].CommandLine.Contains($ChildHelper)) { throw 'surviving child executable/helper identity was not exact' }`,
        `    [ordered]@{ pid=[int]$childPostCap[0].ProcessId; executablePath=[string]$childPostCap[0].ExecutablePath; commandLine=[string]$childPostCap[0].CommandLine; creationFileTime=$childCreationFileTime.ToString() } | ConvertTo-Json -Compress | Out-File -LiteralPath $ChildIdentityFile -Encoding utf8`,
        `  } finally { $childProcess.Dispose() }`,
        `  $waitStarted = [Diagnostics.Stopwatch]::GetTimestamp()`,
        `  $timedOut = $false`,
        `  try {`,
        `    $wait = Wait-FootprintJobProcess $launch.process $launch.boundary 2000 200`,
        `    throw ('false successful root receipt exit=' + $wait.exitCode)`,
        `  } catch {`,
        `    $msg = $_.Exception.Message`,
        `    if ($msg -match 'false successful') { throw }`,
        `    if ($msg -notmatch 'timed out') { throw ('unexpected wait failure: ' + $msg) }`,
        `    $timedOut = $true`,
        `  }`,
        `  $waitElapsedMs = [int](([Diagnostics.Stopwatch]::GetTimestamp() - $waitStarted) * 1000 / [Diagnostics.Stopwatch]::Frequency)`,
        `  $stop = Stop-FootprintJobProcess $launch.process $launch.boundary 0 1000`,
        `  if (-not $stop.treeBoundaryVerified) { throw 'tree boundary not verified' }`,
        `  if ($stop.orphanProcessesAfterClose -ne 0) { throw 'orphan not zero after forced cleanup' }`,
        `  [ordered]@{ childLive = $true; timedOut = $timedOut; waitElapsedMs = $waitElapsedMs; forced = [bool]$stop.forced; verified = $stop.treeBoundaryVerified; orphan = $stop.orphanProcessesAfterClose } | ConvertTo-Json -Compress`,
        `} finally { try { $launch.boundary.Dispose() } catch {} ; try { if ($null -ne $launch.process) { $launch.process.Dispose() } } catch {} }`,
      ].join("\n"),
    );
    const stdout = execFileSync(
      "powershell.exe",
      [
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        driver,
        modulePath,
        completionModulePath,
        process.execPath,
        rootHelper,
        childHelper,
        childPidFile,
        childReady,
        rootPidFile,
        rootIdentity,
        childIdentity,
      ],
      { encoding: "utf8", timeout: 20_000, windowsHide: true },
    );
    const observed = JSON.parse(stdout.trim());
    assert.equal(observed.childLive, true);
    assert.equal(observed.timedOut, true);
    assert.ok(observed.waitElapsedMs >= 1_500);
    assert.ok(observed.waitElapsedMs < 10_000);
    assert.equal(typeof observed.forced, "boolean");
    assert.equal(observed.verified, true);
    assert.equal(observed.orphan, 0);
  } finally {
    cleanRoots(root, [
      {
        pidFiles: [rootPidFile],
        identityFile: rootIdentity,
        label: "surviving-child root",
        commandParts: [rootHelper],
      },
      {
        pidFiles: [childPidFile, childSpawned],
        identityFile: childIdentity,
        label: "surviving-child child",
        commandParts: [childHelper],
      },
    ]);
  }
});

test("native Job unassigned startup failure reaps its suspended root", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-windows-job-"));
  const pidFile = path.join(root, "unassigned.pid");
  const identityFile = path.join(root, "unassigned.identity.json");
  const nonce = `sage-nonce-${Date.now()}-${Math.floor(Math.random() * 1000000)}`;
  let helper;
  try {
    helper = path.join(root, "startup-never-runs.mjs");
    writeFileSync(helper, `process.exit(0);\n`);
    const modulePath = path.resolve("scripts/footprint/WindowsJob.psm1");
    const driver = path.join(root, "driver-unassigned.ps1");
    writeFileSync(
      driver,
      [
        `param([string]$ModulePath,[string]$NodePath,[string]$Helper,[string]$FixtureRoot,[string]$Nonce,[string]$PidFile,[string]$IdentityFile)`,
        `[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)`,
        `$moduleSource = [IO.File]::ReadAllText($ModulePath)`,
        `$match = [regex]::Match($moduleSource, "(?s)Add-Type -TypeDefinition @'\\r?\\n(.*?)\\r?\\n'@")`,
        `if (-not $match.Success) { throw 'could not extract production C# source' }`,
        `$testSource = "#define SAGE_FOOTPRINT_JOB_TESTS" + [Environment]::NewLine + $match.Groups[1].Value`,
        `Add-Type -TypeDefinition $testSource -ErrorAction Stop`,
        `if (-not ("SageFootprint.NativeJobBoundary" -as [type])) { throw 'test native type was unavailable after Add-Type' }`,
        `Import-Module $ModulePath -Force -ErrorAction Stop`,
        `$proof = [SageFootprint.NativeJobBoundary]::StartForStartupFailureTest($NodePath, @($Helper, $Nonce, $FixtureRoot), $FixtureRoot, @('PATH=' + $env:PATH), 4, 1)`,
        `$pidOut = $proof.ProcessId; $creation = $proof.CreationFileTime`,
        `[IO.File]::WriteAllText($PidFile, $pidOut.ToString())`,
        `[ordered]@{ pid=$pidOut; creationFileTime=$creation.ToString(); executablePath=$proof.ImagePath; commandLine=$proof.CommandLine; nonce=$Nonce } | ConvertTo-Json -Compress | Out-File -LiteralPath $IdentityFile -Encoding utf8`,
        `if (-not $proof.Reaped) { throw 'unassigned root was not reaped by production cleanup' }`,
        `[ordered]@{ pid=$pidOut; creation=$creation; reaped=[bool]$proof.Reaped } | ConvertTo-Json -Compress`,
      ].join("\n"),
    );
    const stdout = execFileSync(
      "powershell.exe",
      [
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        driver,
        modulePath,
        process.execPath,
        helper,
        root,
        nonce,
        pidFile,
        identityFile,
      ],
      { encoding: "utf8", timeout: 20_000, windowsHide: true },
    );
    const observed = JSON.parse(stdout.trim());
    assert.ok(observed.pid > 0);
    assert.ok(observed.creation > 0);
    assert.equal(observed.reaped, true);
  } finally {
    cleanRoots(root, [
      {
        pidFiles: [pidFile],
        identityFile,
        label: "unassigned startup root",
        commandParts: [helper, nonce],
      },
    ]);
  }
});

test("native Job assigned startup failure reaps its suspended root", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-windows-job-"));
  const pidFile = path.join(root, "assigned.pid");
  const identityFile = path.join(root, "assigned.identity.json");
  const nonce = `sage-nonce-${Date.now()}-${Math.floor(Math.random() * 1000000)}`;
  let helper;
  try {
    helper = path.join(root, "startup-never-runs.mjs");
    writeFileSync(helper, `process.exit(0);\n`);
    const modulePath = path.resolve("scripts/footprint/WindowsJob.psm1");
    const driver = path.join(root, "driver-assigned.ps1");
    writeFileSync(
      driver,
      [
        `param([string]$ModulePath,[string]$NodePath,[string]$Helper,[string]$FixtureRoot,[string]$Nonce,[string]$PidFile,[string]$IdentityFile)`,
        `[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)`,
        `$moduleSource = [IO.File]::ReadAllText($ModulePath)`,
        `$match = [regex]::Match($moduleSource, "(?s)Add-Type -TypeDefinition @'\\r?\\n(.*?)\\r?\\n'@")`,
        `if (-not $match.Success) { throw 'could not extract production C# source' }`,
        `$testSource = "#define SAGE_FOOTPRINT_JOB_TESTS" + [Environment]::NewLine + $match.Groups[1].Value`,
        `Add-Type -TypeDefinition $testSource -ErrorAction Stop`,
        `if (-not ("SageFootprint.NativeJobBoundary" -as [type])) { throw 'test native type was unavailable after Add-Type' }`,
        `Import-Module $ModulePath -Force -ErrorAction Stop`,
        `$proof = [SageFootprint.NativeJobBoundary]::StartForStartupFailureTest($NodePath, @($Helper, $Nonce, $FixtureRoot), $FixtureRoot, @('PATH=' + $env:PATH), 4, 2)`,
        `$pidOut = $proof.ProcessId; $creation = $proof.CreationFileTime`,
        `[IO.File]::WriteAllText($PidFile, $pidOut.ToString())`,
        `[ordered]@{ pid=$pidOut; creationFileTime=$creation.ToString(); executablePath=$proof.ImagePath; commandLine=$proof.CommandLine; nonce=$Nonce } | ConvertTo-Json -Compress | Out-File -LiteralPath $IdentityFile -Encoding utf8`,
        `if (-not $proof.Reaped) { throw 'assigned root was not reaped by production cleanup' }`,
        `[ordered]@{ pid=$pidOut; creation=$creation; reaped=[bool]$proof.Reaped } | ConvertTo-Json -Compress`,
      ].join("\n"),
    );
    const stdout = execFileSync(
      "powershell.exe",
      [
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        driver,
        modulePath,
        process.execPath,
        helper,
        root,
        nonce,
        pidFile,
        identityFile,
      ],
      { encoding: "utf8", timeout: 20_000, windowsHide: true },
    );
    const observed = JSON.parse(stdout.trim());
    assert.ok(observed.pid > 0);
    assert.ok(observed.creation > 0);
    assert.equal(observed.reaped, true);
  } finally {
    cleanRoots(root, [
      {
        pidFiles: [pidFile],
        identityFile,
        label: "assigned startup root",
        commandParts: [helper, nonce],
      },
    ]);
  }
});
