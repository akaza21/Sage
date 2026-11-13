import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

const root = path.resolve(".");
const wrapper = path.join(root, "scripts", "run-acp05-owned-launcher.ps1");
const powershell = path.join(
  process.env.SystemRoot,
  "System32",
  "WindowsPowerShell",
  "v1.0",
  "powershell.exe",
);

// Hang guard for each Windows PowerShell run below, not a speed budget. Every
// run compiles the launcher's C# helpers with Add-Type, which starts csc.exe
// from C:. Hosted runners reload Defender security intelligence partway
// through a job, and C: page-ins stall for about 30 s while that happens; a
// cold first compile then takes about 9 s on its own (10.98 s for a whole
// -ValidateOnly run against about 1.2 s warm). A guard of 10 s killed healthy
// runs, so the guard covers that stall with margin.
const POWERSHELL_RUN_GUARD_MS = 60_000;

function gitFixture(prefix) {
  const temp = mkdtempSync(path.join(os.tmpdir(), prefix));
  execFileSync("git", ["init", "--quiet", temp]);
  execFileSync("git", ["-C", temp, "config", "user.name", "Sage test"]);
  execFileSync("git", [
    "-C",
    temp,
    "config",
    "user.email",
    "sage-test@localhost",
  ]);
  writeFileSync(path.join(temp, "README.md"), "fixture\n");
  writeFileSync(path.join(temp, ".gitignore"), "local-evidence/\n*.exe\n");
  execFileSync("git", ["-C", temp, "add", "README.md", ".gitignore"]);
  execFileSync("git", ["-C", temp, "commit", "--quiet", "-m", "fixture"]);
  const executable = path.join(temp, "sage.exe");
  writeFileSync(executable, "fixture");
  const hash = execFileSync(
    powershell,
    [
      "-NoProfile",
      "-Command",
      `(Get-FileHash -LiteralPath '${executable.replaceAll("'", "''")}' -Algorithm SHA256).Hash.ToLowerInvariant()`,
    ],
    { encoding: "utf8" },
  ).trim();
  return {
    temp,
    executable,
    hash,
    commit: execFileSync("git", ["-C", temp, "rev-parse", "HEAD"], {
      encoding: "utf8",
    }).trim(),
  };
}

function runFunctionProbe(names, body, options = {}) {
  const escaped = wrapper.replaceAll("'", "''");
  const wanted = names.map((name) => `'${name}'`).join(",");
  const { ownedTypes = false, ...execOptions } = options;
  const source = readFileSync(wrapper, "utf8");
  const typeMatch = source.match(
    /Add-Type -TypeDefinition @'\r?\n([\s\S]*?)\r?\n'@ \}/u,
  );
  const typePrelude = ownedTypes
    ? `Add-Type -TypeDefinition @'\n${typeMatch?.[1] ?? "missing type definition"}\n'@\n`
    : "";
  const command = [
    "$ErrorActionPreference='Stop'",
    `$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile('${escaped}',[ref]$tokens,[ref]$errors)`,
    "if($errors.Count){throw ($errors|% Message)}",
    `foreach($name in @(${wanted})){$fn=$ast.FindAll({param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name},$true)|Select-Object -First 1;if($null -eq $fn){throw "missing function $name"};. ([scriptblock]::Create($fn.Extent.Text))}`,
    body,
  ].join(";");
  return execFileSync(
    powershell,
    ["-NoProfile", "-NonInteractive", "-Command", `${typePrelude}${command}`],
    { encoding: "utf8", windowsHide: true, ...execOptions },
  );
}

test("ACP-05 owned launcher parses on Windows PowerShell 5.1", () => {
  const escaped = wrapper.replaceAll("'", "''");
  const output = execFileSync(
    powershell,
    [
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      `$t=$null;$e=$null;$null=[Management.Automation.Language.Parser]::ParseFile('${escaped}',[ref]$t,[ref]$e);$e|% Message`,
    ],
    { encoding: "utf8" },
  );
  assert.equal(output.trim(), "");
});

test("ACP-05 owned launcher accepts a clean exact source and exact executable without GUI launch", () => {
  const { temp, executable, hash, commit } = gitFixture("sage-acp05-owned-");
  try {
    const evidenceParent = path.join(temp, "local-evidence");
    const result = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-File",
        wrapper,
        "-RepoRoot",
        temp,
        "-SourceCommit",
        commit,
        "-Executable",
        executable,
        "-ExecutableSha256",
        hash,
        "-ListenerExecutable",
        executable,
        "-ListenerExecutableSha256",
        hash,
        "-CdpPort",
        "9235",
        "-EvidenceRoot",
        path.join(evidenceParent, "run"),
        "-ValidateOnly",
      ],
      { encoding: "utf8", windowsHide: true, timeout: POWERSHELL_RUN_GUARD_MS },
    );
    assert.equal(result.status, 0, result.stderr);
    assert.equal(JSON.parse(result.stdout).executable.sha256, hash);
    assert.equal(JSON.parse(result.stdout).listenerExecutable.sha256, hash);
    assert.equal(existsSync(path.join(temp, "native-stdout.log")), false);
  } finally {
    rmSync(temp, { recursive: true, force: true });
  }
});

test("ACP-05 owned launcher rejects a dirty source before any GUI launch", () => {
  const { temp, executable, hash, commit } = gitFixture(
    "sage-acp05-owned-dirty-",
  );
  try {
    writeFileSync(path.join(temp, "DIRTY.txt"), "untracked\n");
    const result = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-File",
        wrapper,
        "-RepoRoot",
        temp,
        "-SourceCommit",
        commit,
        "-Executable",
        executable,
        "-ExecutableSha256",
        hash,
        "-ListenerExecutable",
        executable,
        "-ListenerExecutableSha256",
        hash,
        "-CdpPort",
        "9235",
        "-EvidenceRoot",
        path.join(temp, "local-evidence", "run"),
        "-ValidateOnly",
      ],
      { encoding: "utf8", windowsHide: true, timeout: POWERSHELL_RUN_GUARD_MS },
    );
    assert.notEqual(result.status, 0);
    assert.match(`${result.stdout}\n${result.stderr}`, /repo must be clean/u);
  } finally {
    rmSync(temp, { recursive: true, force: true });
  }
});

test("fresh-port, absolute-deadline and bounded-log gates fail closed", () => {
  const temp = mkdtempSync(path.join(os.tmpdir(), "sage-acp05-probes-"));
  try {
    const small = path.join(temp, "small.log");
    const large = path.join(temp, "large.log");
    writeFileSync(small, Buffer.alloc(8));
    writeFileSync(large, Buffer.alloc(129));
    const output = runFunctionProbe(
      [
        "Fail",
        "Get-RemainingMilliseconds",
        "Assert-BoundedLog",
        "Assert-FreshCdpPort",
      ],
      "$listener=[Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0);$listener.Start();$port=([Net.IPEndPoint]$listener.LocalEndpoint).Port;$threw=$false;try{Assert-FreshCdpPort $port}catch{$threw=$true};if(-not $threw){throw 'occupied port accepted'};$listener.Stop();Assert-FreshCdpPort $port;$threw=$false;try{Get-RemainingMilliseconds ([Diagnostics.Stopwatch]::GetTimestamp()-1)|Out-Null}catch{$threw=$true};if(-not $threw){throw 'expired deadline accepted'};Assert-BoundedLog $env:SMALL_LOG 8|Out-Null;$threw=$false;try{Assert-BoundedLog $env:LARGE_LOG 128|Out-Null}catch{$threw=$true};if(-not $threw){throw 'oversized log accepted'};'ok'",
      { env: { ...process.env, SMALL_LOG: small, LARGE_LOG: large } },
    );
    assert.equal(output.trim(), "ok");
  } finally {
    rmSync(temp, { recursive: true, force: true });
  }
});

test("listener collision is rejected before any process can be admitted", () => {
  const output = runFunctionProbe(
    ["Fail", "Get-OwnedCdpListener"],
    "function Get-NetTCPConnection{[CmdletBinding()]param($State,$LocalPort);@([pscustomobject]@{LocalAddress='127.0.0.1';OwningProcess=11},[pscustomobject]@{LocalAddress='0.0.0.0';OwningProcess=12})};$threw=$false;try{Get-OwnedCdpListener 9235 99 $null|Out-Null}catch{$threw=$true};if(-not $threw){throw 'listener collision accepted'};'ok'",
  );
  assert.equal(output.trim(), "ok");
});

test("launcher accepts zero-exit collected receipt only as collection-only incomplete", () => {
  const output = runFunctionProbe(
    ["Fail", "Test-Acp05CollectorReceipt"],
    "$binding=[ordered]@{binding='launcher-attested-owned-stdout'};$good=[pscustomobject]@{outcome='collected';evidenceStatus='collection-only-incomplete';marker=[pscustomobject]@{proof='launcher-attested-owned-stdout'}};Test-Acp05CollectorReceipt $good $binding|Out-Null;$bad=[pscustomobject]@{outcome='passed';evidenceStatus='native-complete';marker=[pscustomobject]@{proof='native-pairing-marker'}};$threw=$false;try{Test-Acp05CollectorReceipt $bad $binding|Out-Null}catch{$threw=$true};if(-not $threw){throw 'native result upgraded launcher collection'};'ok'",
  );
  assert.equal(output.trim(), "ok");
});

// The leased image runs as the whole process tree and exits on its own when
// its stdin closes. Launching it with Start-Process gave it a new console, so
// a conhost.exe child (which opens the image while it starts) and a ping.exe
// grandchild outlived Process.Kill() and could still be starting when the
// test deleted the image.
test("deny-write/delete executable lease rejects B-in and blocks launch/restore-A swap", () => {
  const temp = mkdtempSync(path.join(os.tmpdir(), "sage-acp05-lease-"));
  const executable = path.join(temp, "sage.exe");
  try {
    const sourceExecutable = path.join(
      process.env.SystemRoot,
      "System32",
      "cmd.exe",
    );
    copyFileSync(sourceExecutable, executable);
    const hash = execFileSync(
      powershell,
      [
        "-NoProfile",
        "-Command",
        `(Get-FileHash -LiteralPath '${executable.replaceAll("'", "''")}' -Algorithm SHA256).Hash.ToLowerInvariant()`,
      ],
      { encoding: "utf8" },
    ).trim();
    const output = runFunctionProbe(
      [
        "Fail",
        "Assert-PlainAncestorChain",
        "Open-ExactExecutableLease",
        "Get-FileLeaseBinding",
      ],
      "$path=$env:LEASE_EXE;[IO.File]::WriteAllText($path,'B');$threw=$false;try{$bad=Open-ExactExecutableLease $path $env:A_HASH 'image'}catch{$threw=$true};if(-not $threw){$bad.Dispose();throw 'B-in accepted'};[IO.File]::Copy($env:A_SOURCE,$path,$true);$lease=Open-ExactExecutableLease $path $env:A_HASH 'image';$p=$null;try{$start=[Diagnostics.ProcessStartInfo]::new($path,'/d /q /k');$start.UseShellExecute=$false;$start.RedirectStandardInput=$true;$start.RedirectStandardOutput=$true;$p=[Diagnostics.Process]::Start($start);$threw=$false;try{[IO.File]::WriteAllText($path,'B')}catch{$threw=$true};if(-not $threw){throw 'leased launched image was writable'};$threw=$false;try{[IO.File]::Copy($env:A_SOURCE,$path,$true)}catch{$threw=$true};if(-not $threw){throw 'restore-A swap was accepted'};$binding=Get-FileLeaseBinding $lease;if($binding.binding -ne 'deny-write-delete-lease'){throw 'lease not recorded'}}finally{if($null-ne $p){$p.StandardInput.Close();$p.WaitForExit();$p.Dispose()};$lease.Dispose()};[IO.File]::WriteAllText($path,'B');'ok'",
      {
        ownedTypes: true,
        env: {
          ...process.env,
          LEASE_EXE: executable,
          A_SOURCE: sourceExecutable,
          A_HASH: hash,
        },
      },
    );
    assert.equal(output.trim(), "ok");
  } finally {
    rmSync(temp, { recursive: true, force: true });
  }
});

test("staging promotion rejects pre-cleanup state and preserves native identity after terminal zero", () => {
  const temp = mkdtempSync(path.join(os.tmpdir(), "sage-acp05-promote-"));
  try {
    const stage = path.join(temp, ".run.staging-fixture");
    const final = path.join(temp, "run");
    execFileSync(powershell, [
      "-NoProfile",
      "-Command",
      `New-Item -ItemType Directory -Path '${stage.replaceAll("'", "''")}'|Out-Null;Set-Content -LiteralPath '${path.join(stage, "receipt.json").replaceAll("'", "''")}' -Value '{}' -NoNewline`,
    ]);
    const log = path.join(stage, "bounded.log");
    writeFileSync(log, "ok");
    const output = runFunctionProbe(
      [
        "Fail",
        "Assert-PlainAncestorChain",
        "PlainDirectory",
        "Assert-NewDirectChild",
        "Get-NativeDirectoryIdentity",
        "Open-DirectoryLease",
        "Assert-DirectoryLease",
        "Assert-BoundedLog",
        "Assert-Acp05LogSet",
        "Assert-Acp05CleanupGate",
        "Publish-Acp05Stage",
      ],
      "$parentLease=Open-DirectoryLease $env:PARENT;try{$stageIdentity=Get-NativeDirectoryIdentity $env:STAGE;$logs=[ordered]@{$env:LOG=8};$zero=[ordered]@{treeBoundaryVerified=$true;orphanProcessesAfterClose=0};$bad=[ordered]@{collector=$null;sage=$zero;portReleased=$true;executableAbsent=$true};$threw=$false;try{Publish-Acp05Stage $env:STAGE $env:FINAL $env:PARENT $parentLease $stageIdentity $bad $logs|Out-Null}catch{$threw=$true};if(-not $threw -or -not(Test-Path -LiteralPath $env:STAGE) -or (Test-Path -LiteralPath $env:FINAL)){throw 'pre-cleanup promotion accepted'};$good=[ordered]@{collector=$zero;sage=$zero;portReleased=$true;executableAbsent=$true};$result=Publish-Acp05Stage $env:STAGE $env:FINAL $env:PARENT $parentLease $stageIdentity $good $logs;if(Test-Path -LiteralPath $env:STAGE){throw 'stage survived'};if(-not(Test-Path -LiteralPath(Join-Path $result 'receipt.json'))){throw 'receipt missing'};$after=Get-NativeDirectoryIdentity $result;if($after.token -ne $stageIdentity.token){throw 'directory identity changed'};'ok'}finally{$parentLease.Dispose()}",
      {
        ownedTypes: true,
        env: {
          ...process.env,
          STAGE: stage,
          FINAL: final,
          PARENT: temp,
          LOG: log,
        },
      },
    );
    assert.equal(output.trim(), "ok");
    assert.equal(existsSync(stage), false);
    assert.equal(existsSync(path.join(final, "receipt.json")), true);
  } finally {
    rmSync(temp, { recursive: true, force: true });
  }
});

test("log cap is rechecked after Job terminal zero", () => {
  const temp = mkdtempSync(path.join(os.tmpdir(), "sage-acp05-exit-cap-"));
  const log = path.join(temp, "exit.log");
  writeFileSync(log, "");
  try {
    const module = path.join(root, "scripts", "footprint", "WindowsJob.psm1");
    const output = runFunctionProbe(
      [
        "Fail",
        "Get-RemainingMilliseconds",
        "Assert-BoundedLog",
        "Assert-Acp05LogSet",
        "Stop-Acp05OwnedJob",
        "Stop-Acp05OwnedJobAndCheckLogs",
      ],
      "$module=$env:JOB_MODULE;Import-Module $module -Force;$info=[Diagnostics.ProcessStartInfo]::new();$info.FileName=$env:PS_EXE;$info.WorkingDirectory=$env:TEMP;$info.UseShellExecute=$false;$info.Arguments='';$command='[IO.File]::WriteAllBytes($env:EXIT_LOG,[byte[]]::new(129))';$launch=Start-FootprintJobProcess -StartInfo $info -ArgumentList @('-NoProfile','-NonInteractive','-Command',$command) -ActiveProcessLimit 2;$launch.process.WaitForExit(3000)|Out-Null;$deadline=[Diagnostics.Stopwatch]::GetTimestamp()+[long](5*[Diagnostics.Stopwatch]::Frequency);$threw=$false;try{Stop-Acp05OwnedJobAndCheckLogs $launch $deadline ([ordered]@{$env:EXIT_LOG=128})|Out-Null}catch{$threw=$true};if(-not $threw){throw 'cap-at-exit accepted'};'ok'",
      {
        env: {
          ...process.env,
          JOB_MODULE: module,
          PS_EXE: powershell,
          EXIT_LOG: log,
        },
        timeout: POWERSHELL_RUN_GUARD_MS,
      },
    );
    assert.equal(output.trim(), "ok");
  } finally {
    rmSync(temp, { recursive: true, force: true });
  }
});

test("introduced junction forces identity-preserving same-parent quarantine without traversal", () => {
  const temp = mkdtempSync(path.join(os.tmpdir(), "sage-acp05-quarantine-"));
  const stage = path.join(temp, ".run.staging-fixture");
  const outside = mkdtempSync(path.join(os.tmpdir(), "sage-acp05-outside-"));
  const outsideFile = path.join(outside, "keep.txt");
  try {
    writeFileSync(outsideFile, "keep");
    execFileSync(powershell, [
      "-NoProfile",
      "-Command",
      `New-Item -ItemType Directory -Path '${stage.replaceAll("'", "''")}'|Out-Null;New-Item -ItemType Junction -Path '${path.join(stage, "escape").replaceAll("'", "''")}' -Target '${outside.replaceAll("'", "''")}'|Out-Null`,
    ]);
    const output = runFunctionProbe(
      [
        "Fail",
        "Assert-PlainAncestorChain",
        "PlainDirectory",
        "Assert-NewDirectChild",
        "Get-NativeDirectoryIdentity",
        "Open-DirectoryLease",
        "Assert-DirectoryLease",
        "Remove-OwnedStage",
        "Quarantine-Acp05Stage",
      ],
      "$lease=Open-DirectoryLease $env:PARENT;try{$identity=Get-NativeDirectoryIdentity $env:STAGE;$threw=$false;try{Remove-OwnedStage $env:STAGE $env:PARENT $lease $identity '.run.staging-'}catch{$threw=$true};if(-not $threw){throw 'reparse stage removed'};$bad=[ordered]@{path=$identity.path;token='wrong'};$threw=$false;try{Quarantine-Acp05Stage $env:STAGE $env:PARENT $lease $bad 'run' 'bad'|Out-Null}catch{$threw=$true};if(-not $threw -or -not(Test-Path -LiteralPath $env:STAGE)){throw 'bad quarantine identity accepted'};$renamed=Quarantine-Acp05Stage $env:STAGE $env:PARENT $lease $identity 'run' 'fixture';$after=Get-NativeDirectoryIdentity $renamed;if($after.token -ne $identity.token -or (Test-Path -LiteralPath $env:STAGE)){throw 'quarantine identity changed'};'ok'}finally{$lease.Dispose()}",
      { ownedTypes: true, env: { ...process.env, STAGE: stage, PARENT: temp } },
    );
    assert.equal(output.trim(), "ok");
    assert.equal(existsSync(outsideFile), true);
    assert.equal(existsSync(path.join(temp, "run")), false);
  } finally {
    rmSync(temp, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});

test("owned Job cleanup reaches a verified terminal zero within the caller deadline", () => {
  const module = path.join(root, "scripts", "footprint", "WindowsJob.psm1");
  const output = runFunctionProbe(
    ["Fail", "Get-RemainingMilliseconds", "Stop-Acp05OwnedJob"],
    `$module=$env:JOB_MODULE;Import-Module $module -Force;$info=[Diagnostics.ProcessStartInfo]::new();$info.FileName=$env:PS_EXE;$info.WorkingDirectory=$env:TEMP;$info.UseShellExecute=$false;$info.Arguments='';$command='$child=Start-Process powershell.exe -ArgumentList @(''-NoProfile'',''-NonInteractive'',''-Command'',''Start-Sleep -Seconds 30'') -PassThru;Start-Sleep -Seconds 30';$launch=Start-FootprintJobProcess -StartInfo $info -ArgumentList @('-NoProfile','-NonInteractive','-Command',$command) -ActiveProcessLimit 3;Start-Sleep -Milliseconds 300;$deadline=[Diagnostics.Stopwatch]::GetTimestamp()+[long](5*[Diagnostics.Stopwatch]::Frequency);$result=Stop-Acp05OwnedJob $launch $deadline;if(-not $result.treeBoundaryVerified -or $result.orphanProcessesAfterClose -ne 0){throw 'not terminal zero'};'ok'`,
    {
      env: { ...process.env, JOB_MODULE: module, PS_EXE: powershell },
      timeout: POWERSHELL_RUN_GUARD_MS,
    },
  );
  assert.equal(output.trim(), "ok");
});
