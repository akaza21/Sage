import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import os from "node:os";
import path from "node:path";
import test from "node:test";

const powershell = path.join(
  process.env.SystemRoot ?? "C:\\Windows",
  "System32",
  "WindowsPowerShell",
  "v1.0",
  "powershell.exe",
);
const identityModule = path.resolve(
  "scripts",
  "footprint",
  "ProcessIdentity.psm1",
);

function runPowerShellFixture(body, { timeout = 10_000 } = {}) {
  const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;Import-Module $env:SAGE_PROCESS_IDENTITY_MODULE -Force;${body}`;
  const result = spawnSync(
    powershell,
    [
      "-NoProfile",
      "-NonInteractive",
      "-EncodedCommand",
      Buffer.from(source, "utf16le").toString("base64"),
    ],
    {
      encoding: "utf8",
      env: {
        ...process.env,
        SAGE_PROCESS_IDENTITY_MODULE: identityModule,
      },
      windowsHide: true,
      timeout,
    },
  );
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
  return JSON.parse(result.stdout.trim());
}

const commonFixture = String.raw`
$root=[ordered]@{processId=101;executablePath='C:\Apps\vite.cmd';creationDate='2026-08-30T00:00:00.000Z'};
$rootRow=[pscustomobject]@{ProcessId=101;ParentProcessId=1;ExecutablePath='C:\Apps\vite.cmd';CreationDate='2026-08-30T00:00:00.000Z'};
$childRow=[pscustomobject]@{ProcessId=202;ParentProcessId=101;ExecutablePath='C:\Apps\node.exe';CreationDate='2026-08-30T00:00:01.000Z'};
$reusedRootRow=[pscustomobject]@{ProcessId=101;ParentProcessId=1;ExecutablePath='C:\Windows\System32\notepad.exe';CreationDate='2026-08-30T00:01:00.000Z'};
$store=New-FootprintIdentityStore;
[void](Add-FootprintRememberedIdentity $store $rootRow);
function Invoke-TestFootprintTree($rootIdentity,$knownIdentities,$snapshotProvider,$stopProcessById,$utcNow,$delay){
  $module=Get-Module ProcessIdentity;
  return & $module {
    param($rootValue,$knownValue,$snapshotValue,$stopValue,$clockValue,$delayValue)
    Invoke-FootprintTreeCore $rootValue $knownValue 1 $snapshotValue $stopValue $clockValue $delayValue
  } $rootIdentity $knownIdentities $snapshotProvider $stopProcessById $utcNow $delay
};
`;

test("duplicate PID rows fail closed for roots and remembered children", () => {
  const observed = runPowerShellFixture(`${commonFixture}
[void](Add-FootprintRememberedIdentity $store $childRow);
$replacementChild=$childRow.PSObject.Copy();$replacementChild.CreationDate=$reusedRootRow.CreationDate;
$snapshots=@(
  @($rootRow,$rootRow,$childRow),
  @($reusedRootRow,$rootRow,$childRow),
  @($rootRow,$childRow,$childRow),
  @($rootRow,$replacementChild,$childRow)
);
$results=@(foreach($rows in $snapshots){
  $plan=Get-FootprintTeardownPlan $root $store $rows;
  [ordered]@{rootState=$plan.rootState;identityLost=$plan.identityLost;candidates=@($plan.candidates|ForEach-Object ProcessId);reused=@($plan.reusedProcessIds)}
});
ConvertTo-Json -InputObject $results -Compress
`);
  assert.deepEqual(observed, [
    { rootState: "lost", identityLost: true, candidates: [202], reused: [] },
    { rootState: "lost", identityLost: true, candidates: [202], reused: [] },
    {
      rootState: "matching",
      identityLost: true,
      candidates: [101],
      reused: [],
    },
    {
      rootState: "matching",
      identityLost: true,
      candidates: [101],
      reused: [],
    },
  ]);
});

test("post-kill root PID reuse is audit-only after an exact successful root stop", () => {
  const observed = runPowerShellFixture(`${commonFixture}
[void](Add-FootprintRememberedIdentity $store $childRow);
$script:rootAlive=$true;$script:childAlive=$true;$script:rootStopped=$false;$script:now=[DateTime]::Parse('2026-08-30T00:00:00Z');$script:stops=@();
$snapshot={
  $rows=@();
  if($script:rootAlive){$rows+=$rootRow}elseif($script:rootStopped){$rows+=$reusedRootRow};
  if($script:childAlive){$rows+=$childRow};
  return $rows
};
$stop={param($identity)$processId=[int]$identity.processId;$script:stops+=$processId;if($processId-eq202){$script:childAlive=$false}elseif($processId-eq101){$script:rootAlive=$false;$script:rootStopped=$true}else{throw 'unexpected process'};$true};
$clock={return $script:now};$delay={param([int]$milliseconds)$script:now=$script:now.AddSeconds(2)};
$result=Invoke-TestFootprintTree $root $store $snapshot $stop $clock $delay;
[ordered]@{identityLost=$result.identityLost;remaining=@($result.remaining);reused=@($result.reusedProcessIds);stops=@($script:stops)}|ConvertTo-Json -Compress
`);
  assert.deepEqual(observed, {
    identityLost: false,
    remaining: [],
    reused: [101],
    stops: [202, 101],
  });
});

test("root PID reuse proves exit before exact root termination without killing its replacement", () => {
  const observed = runPowerShellFixture(`${commonFixture}
$script:now=[DateTime]::Parse('2026-08-30T00:00:00Z');$script:stops=@();
$snapshot={return @($reusedRootRow)};
$stop={param($identity)$processId=[int]$identity.processId;$script:stops+=$processId;$true};
$clock={return $script:now};$delay={param([int]$milliseconds)$script:now=$script:now.AddSeconds(2)};
$result=Invoke-TestFootprintTree $root $store $snapshot $stop $clock $delay;
[ordered]@{identityLost=$result.identityLost;remaining=@($result.remaining);reused=@($result.reusedProcessIds);stops=@($script:stops)}|ConvertTo-Json -Compress
`);
  assert.deepEqual(observed, {
    identityLost: false,
    remaining: [],
    reused: [101],
    stops: [],
  });
});

test("remembered child PID reuse proves exit after the root is absent", () => {
  const observed = runPowerShellFixture(`${commonFixture}
[void](Add-FootprintRememberedIdentity $store $childRow);
$replacement=[pscustomobject]@{ProcessId=202;ParentProcessId=1;ExecutablePath=$childRow.ExecutablePath;CreationDate='2026-08-30T00:02:00.000Z'};
$snapshot={return @($replacement)};
$result=Invoke-TestFootprintTree $root $store $snapshot {throw 'replacement must not be stopped'} {[DateTime]::UtcNow} {};
[ordered]@{identityLost=$result.identityLost;remaining=@($result.remaining);reused=@($result.reusedProcessIds)}|ConvertTo-Json -Compress
`);
  assert.deepEqual(observed, {
    identityLost: false,
    remaining: [],
    reused: [202],
  });
});

test("an unavailable creation time or executable identity cannot prove exit", () => {
  const observed = runPowerShellFixture(`${commonFixture}
[void](Add-FootprintRememberedIdentity $store $childRow);
$results=@();
foreach($missing in @('CreationDate','ExecutablePath')){
  $row=[pscustomobject]@{ProcessId=202;ParentProcessId=1;ExecutablePath=$childRow.ExecutablePath;CreationDate=$childRow.CreationDate};
  $row.$missing=$null;
  $script:now=[DateTime]::Parse('2026-08-30T00:00:00Z');
  $clock={return $script:now};$delay={param([int]$milliseconds)$script:now=$script:now.AddSeconds(2)};
  $result=Invoke-TestFootprintTree $root $store {return @($row)} {throw 'unproven identity must not be stopped'} $clock $delay;
  $results+=,[ordered]@{identityLost=$result.identityLost;remaining=@($result.remaining);reused=@($result.reusedProcessIds)}
};
ConvertTo-Json -InputObject $results -Compress
`);
  assert.deepEqual(observed, [
    { identityLost: true, remaining: [], reused: [] },
    { identityLost: true, remaining: [], reused: [] },
  ]);
});

test("a process snapshot query failure cannot establish terminal zero", () => {
  const observed = runPowerShellFixture(`${commonFixture}
$failed=$false;
try{[void](Invoke-TestFootprintTree $root $store {throw [UnauthorizedAccessException]::new('snapshot query denied')} {throw 'must not stop'} {[DateTime]::UtcNow} {})}catch{$failed=$_.Exception.Message -eq 'snapshot query denied'};
[ordered]@{failed=$failed}|ConvertTo-Json -Compress
`);
  assert.deepEqual(observed, { failed: true });
});

test("a later snapshot can prove an initially unobservable process exited", () => {
  const observed = runPowerShellFixture(`${commonFixture}
$results=@();
foreach($mode in @('absent','reused')){
  $unknown=[pscustomobject]@{ProcessId=101;ParentProcessId=1;ExecutablePath=$rootRow.ExecutablePath;CreationDate=$null};
  $script:queries=0;
  $snapshot={$script:queries++;if($script:queries-le2){return @($unknown)};if($mode-eq'reused'){return @($reusedRootRow)};return @()};
  $result=Invoke-TestFootprintTree $root $store $snapshot {throw 'must not stop'} {[DateTime]::UtcNow} {};
  $results+=,[ordered]@{identityLost=$result.identityLost;remaining=@($result.remaining);reused=@($result.reusedProcessIds)}
};
ConvertTo-Json -InputObject $results -Compress
`);
  assert.deepEqual(observed, [
    { identityLost: false, remaining: [], reused: [] },
    { identityLost: false, remaining: [], reused: [101] },
  ]);
});

test("post-kill teardown waits for exit proof when a snapshot temporarily loses identity", () => {
  const observed = runPowerShellFixture(`${commonFixture}
$script:now=[DateTime]::Parse('2026-08-30T00:00:00Z');$script:stopped=$false;$script:afterStop=0;
$unknown=[pscustomobject]@{ProcessId=101;ParentProcessId=1;ExecutablePath=$null;CreationDate=$rootRow.CreationDate};
$snapshot={if(-not $script:stopped){return @($rootRow)};$script:afterStop++;if($script:afterStop-eq1){return @($unknown)};return @()};
$stop={param($identity)$script:stopped=$true;$true};
$clock={return $script:now};$delay={param([int]$milliseconds)$script:now=$script:now.AddSeconds(2)};
$result=Invoke-TestFootprintTree $root $store $snapshot $stop $clock $delay;
[ordered]@{forced=$result.forced;identityLost=$result.identityLost;remaining=@($result.remaining);reused=@($result.reusedProcessIds)}|ConvertTo-Json -Compress
`);
  assert.deepEqual(observed, {
    forced: true,
    identityLost: false,
    remaining: [],
    reused: [],
  });
});

test("a recovered root remembers its visible descendants before termination", () => {
  const observed = runPowerShellFixture(`${commonFixture}
$script:now=[DateTime]::Parse('2026-08-30T00:00:00Z');$script:queries=0;$script:rootAlive=$true;
$unknown=[pscustomobject]@{ProcessId=101;ParentProcessId=1;ExecutablePath=$null;CreationDate=$rootRow.CreationDate};
$snapshot={$script:queries++;if(-not $script:rootAlive){return @($childRow)};if($script:queries-le2){return @($unknown,$childRow)};return @($rootRow,$childRow)};
$stop={param($identity)if([int]$identity.processId-eq101){$script:rootAlive=$false};$true};
$clock={return $script:now};$delay={param([int]$milliseconds)$script:now=$script:now.AddSeconds(2)};
$result=Invoke-TestFootprintTree $root $store $snapshot $stop $clock $delay;
[ordered]@{identityLost=$result.identityLost;remaining=@($result.remaining)}|ConvertTo-Json -Compress
`);
  assert.deepEqual(observed, { identityLost: false, remaining: [202] });
});

test("remembered descendants do not require fresh admission while exiting", () => {
  const observed = runPowerShellFixture(`${commonFixture}
$script:now=[DateTime]::Parse('2026-08-30T00:00:00Z');$script:childStopped=$false;$script:rootStopped=$false;
$unknownChild=[pscustomobject]@{ProcessId=202;ParentProcessId=101;ExecutablePath=$null;CreationDate=$childRow.CreationDate};
$snapshot={if($script:rootStopped){return @()};if($script:childStopped){return @($rootRow,$unknownChild)};return @($rootRow,$childRow)};
$stop={param($identity)if([int]$identity.processId-eq202){$script:childStopped=$true}else{$script:rootStopped=$true};$true};
$clock={return $script:now};$delay={param([int]$milliseconds)$script:now=$script:now.AddSeconds(2)};
$result=Invoke-TestFootprintTree $root $store $snapshot $stop $clock $delay;
[ordered]@{forced=$result.forced;identityLost=$result.identityLost;remaining=@($result.remaining)}|ConvertTo-Json -Compress
`);
  assert.deepEqual(observed, {
    forced: true,
    identityLost: false,
    remaining: [],
  });
});

test("a Vite snapshot query failure leaves the live process untouched and blocks receipt promotion", () => {
  const observed = runPowerShellFixture(String.raw`
$wrapper=Join-Path (Get-Location) 'scripts/run-source-functional-native-ide-journey.ps1';
$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile($wrapper,[ref]$tokens,[ref]$errors);
foreach($name in @('Get-CleanupRootTuple','Resolve-CleanupFailureClass','Update-OwnedRuntimeTree','Write-RuntimeCleanupQuarantine','Stop-OwnedRuntimeTree','Publish-PendingJourneyReceipt')){
  $fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);
  Invoke-Expression $fn.Extent.Text
};
$evidence=Join-Path ([IO.Path]::GetTempPath()) ('sage-vite-query-failure-'+[Guid]::NewGuid().ToString('N'));
[void][IO.Directory]::CreateDirectory($evidence);
$powershell=(Get-Process -Id $PID).Path;
$child=Start-Process -FilePath $powershell -ArgumentList @('-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 30') -WindowStyle Hidden -PassThru;
try{
  $identity=Get-FootprintLaunchIdentity $child $powershell;
  $runtime=[ordered]@{process=$child;identity=$identity;store=(New-FootprintIdentityStore)};
  $pending=Join-Path $evidence 'machine-receipt.pending.json';$final=Join-Path $evidence 'machine-receipt.json';
  [IO.File]::WriteAllText($pending,'{}');
  & (Get-Module ProcessIdentity) {function script:Get-CimInstance {[CmdletBinding()]param($ClassName,$Property) Write-Error -Exception ([UnauthorizedAccessException]::new('snapshot query denied'))}};
  $failed=$false;$cleanup=$null;
  try{$cleanup=Stop-OwnedRuntimeTree 'vite' $runtime $evidence}catch{$failed=$_.Exception.Message -eq 'The vite runtime cleanup could not be proven.'};
  $published=$true;try{Publish-PendingJourneyReceipt $pending $final $cleanup $cleanup}catch{$published=$false};
  $child.Refresh();
  $record=Get-Content -LiteralPath (Join-Path $evidence 'runtime-cleanup-quarantine-vite.json') -Raw|ConvertFrom-Json;
  [ordered]@{failed=$failed;alive=(-not $child.HasExited);published=$published;pending=(Test-Path -LiteralPath $pending);final=(Test-Path -LiteralPath $final);stage=$record.stage;identityLost=$record.identityLost;remainingCount=$record.remainingCount}|ConvertTo-Json -Compress
}finally{
  $child.Refresh();if(-not $child.HasExited){$child.Kill();[void]$child.WaitForExit(5000)};
  Remove-Item -LiteralPath $evidence -Recurse -Force
}
`);
  assert.deepEqual(observed, {
    failed: true,
    alive: true,
    published: false,
    pending: true,
    final: false,
    stage: "stop-tree",
    identityLost: null,
    remainingCount: null,
  });
});

test("an exact remembered descendant that survives root termination remains nonzero", () => {
  const observed = runPowerShellFixture(`${commonFixture}
[void](Add-FootprintRememberedIdentity $store $childRow);
$script:rootAlive=$true;$script:rootStopped=$false;$script:now=[DateTime]::Parse('2026-08-30T00:00:00Z');$script:stops=@();
$snapshot={
  $rows=@($childRow);
  if($script:rootAlive){$rows+=$rootRow}elseif($script:rootStopped){$rows+=$reusedRootRow};
  return $rows
};
$stop={param($identity)$processId=[int]$identity.processId;$script:stops+=$processId;if($processId-eq101){$script:rootAlive=$false;$script:rootStopped=$true};$true};
$clock={return $script:now};$delay={param([int]$milliseconds)$script:now=$script:now.AddSeconds(6)};
$result=Invoke-TestFootprintTree $root $store $snapshot $stop $clock $delay;
[ordered]@{identityLost=$result.identityLost;remaining=@($result.remaining);reused=@($result.reusedProcessIds);stops=@($script:stops)}|ConvertTo-Json -Compress
`);
  assert.deepEqual(observed, {
    identityLost: false,
    remaining: [202],
    reused: [101],
    stops: [202, 101],
  });
});

test("a failed exact root stop cannot establish post-kill reuse authority", () => {
  const observed = runPowerShellFixture(`${commonFixture}
$script:now=[DateTime]::Parse('2026-08-30T00:00:00Z');$script:stops=@();
$snapshot={return @($rootRow)};
$stop={param($identity)$processId=[int]$identity.processId;$script:stops+=$processId;throw 'injected stop failure'};
$clock={return $script:now};$delay={param([int]$milliseconds)$script:now=$script:now.AddSeconds(2)};
$failed=$false;try{[void](Invoke-TestFootprintTree $root $store $snapshot $stop $clock $delay)}catch{$failed=$_.Exception.Message -match 'injected stop failure'};
[ordered]@{failed=$failed;stops=@($script:stops)}|ConvertTo-Json -Compress
`);
  assert.deepEqual(observed, { failed: true, stops: [101] });
});

test("a matching candidate that exits between revalidation and stop reaches terminal zero", () => {
  const observed = runPowerShellFixture(`${commonFixture}
$script:rootAlive=$true;$script:now=[DateTime]::Parse('2026-08-30T00:00:00Z');$script:stops=@();
$snapshot={if($script:rootAlive){return @($rootRow)};return @()};
$stop={param($identity)$processId=[int]$identity.processId;$script:stops+=$processId;$script:rootAlive=$false;throw 'process not found'};
$clock={return $script:now};$delay={param([int]$milliseconds)$script:now=$script:now.AddSeconds(2)};
$result=Invoke-TestFootprintTree $root $store $snapshot $stop $clock $delay;
[ordered]@{identityLost=$result.identityLost;remaining=@($result.remaining);reused=@($result.reusedProcessIds);stops=@($script:stops)}|ConvertTo-Json -Compress
`);
  assert.deepEqual(observed, {
    identityLost: false,
    remaining: [],
    reused: [],
    stops: [101],
  });
});

test("an older process that names a reused root PID as its parent is never adopted or stopped", () => {
  const observed = runPowerShellFixture(`${commonFixture}
$orphanRow=[pscustomobject]@{ProcessId=303;ParentProcessId=101;ExecutablePath='C:\\Windows\\System32\\svchost.exe';CreationDate='2026-08-29T23:59:00.000Z'};
$descendants=@(Get-FootprintDescendants 101 @($rootRow,$childRow,$orphanRow) | ForEach-Object { [int]$_.ProcessId });
$script:rootAlive=$true;$script:now=[DateTime]::Parse('2026-08-30T00:00:00Z');$script:stops=@();
$snapshot={$rows=@($orphanRow);if($script:rootAlive){$rows+=$rootRow};return $rows};
$stop={param($identity)$processId=[int]$identity.processId;$script:stops+=$processId;if($processId-eq101){$script:rootAlive=$false};$true};
$clock={return $script:now};$delay={param([int]$milliseconds)$script:now=$script:now.AddSeconds(2)};
$result=Invoke-TestFootprintTree $root $store $snapshot $stop $clock $delay;
[ordered]@{descendants=$descendants;identityLost=$result.identityLost;remaining=@($result.remaining);stops=@($script:stops)}|ConvertTo-Json -Compress
`);
  assert.deepEqual(observed, {
    descendants: [101, 202],
    identityLost: false,
    remaining: [],
    stops: [101],
  });
});

test("a process with unknown creation never vouches for its children or grants partial cleanup authority", () => {
  const observed = runPowerShellFixture(String.raw`
$rootId=[int]::MaxValue;$orphanId=$rootId-2;$childId=$rootId-4;
$started=[DateTime]::Parse('2026-08-30T00:00:00Z');
$rows=@(
  [pscustomobject]@{ProcessId=$rootId;ParentProcessId=1;ExecutablePath='C:\Apps\sage.exe';CreationDate=$started},
  [pscustomobject]@{ProcessId=$orphanId;ParentProcessId=$rootId;ExecutablePath='';CreationDate=$null},
  [pscustomobject]@{ProcessId=$childId;ParentProcessId=$orphanId;ExecutablePath='C:\Windows\System32\svchost.exe';CreationDate=$started.AddSeconds(5)}
);
$descendants=@(Get-FootprintDescendants $rootId $rows | ForEach-Object { [int]$_.ProcessId });
$staged=New-FootprintIdentityStore;
$stagedFailed=$false;try{Add-FootprintRememberedIdentities $staged @($rows[2],$rows[1])}catch{$stagedFailed=$true};
$script:exited=$false;
$process=[pscustomobject]@{Id=$rootId;Path='C:\Apps\sage.exe';MainWindowHandle=[IntPtr]::Zero;StartTime=$started};
$process|Add-Member -MemberType ScriptProperty -Name HasExited -Value {$script:exited};
$process|Add-Member -MemberType ScriptMethod -Name Refresh -Value {};
$process|Add-Member -MemberType ScriptMethod -Name WaitForExit -Value {param([int]$milliseconds) return $script:exited};
$process|Add-Member -MemberType ScriptMethod -Name Kill -Value {param([AllowNull()][object]$entireTree) if($entireTree){throw 'tree kill unavailable'};$script:exited=$true};
$known=New-FootprintIdentityStore;
$boundary='';try{[void](Stop-FootprintLaunchedProcessObject $process 'C:\Apps\sage.exe' $known $rows 0 100)}catch{$boundary=$_.Exception.Message};
[ordered]@{descendants=$descendants;stagedFailed=$stagedFailed;stagedCount=$staged.Count;rootExited=$script:exited;rememberedCount=$known.Count;boundaryRefused=$boundary.StartsWith('Exact launched process-object cleanup could not establish a process-tree boundary.')}|ConvertTo-Json -Compress
`);
  assert.deepEqual(observed, {
    descendants: [2147483645, 2147483647],
    stagedFailed: true,
    stagedCount: 0,
    rootExited: true,
    rememberedCount: 0,
    boundaryRefused: true,
  });
});

test("descendants need a proven root creation time, and a reused root PID vouches for nothing", () => {
  const observed = runPowerShellFixture(String.raw`
$started=[DateTime]::Parse('2026-08-30T00:00:00Z').ToUniversalTime();
$ids={param($rows,$creation) $arguments=@{RootProcessId=101;ProcessRows=$rows};if($null -ne $creation){$arguments.RootCreationDate=$creation};@(Get-FootprintDescendants @arguments | ForEach-Object { [int]$_.ProcessId })};
$exited=@(
  [pscustomobject]@{ProcessId=303;ParentProcessId=101;ExecutablePath='C:\Windows\System32\svchost.exe';CreationDate=$started.AddMinutes(-1)},
  [pscustomobject]@{ProcessId=202;ParentProcessId=101;ExecutablePath='C:\Apps\node.exe';CreationDate=$started.AddSeconds(1)}
);
$live=@(
  [pscustomobject]@{ProcessId=101;ParentProcessId=1;ExecutablePath='C:\Apps\vite.cmd';CreationDate=$started},
  [pscustomobject]@{ProcessId=202;ParentProcessId=101;ExecutablePath='C:\Apps\node.exe';CreationDate=$started.AddSeconds(1)}
);
[ordered]@{
  exitedWithoutLaunchTime=@(& $ids $exited $null);
  exitedWithLaunchTime=@(& $ids $exited '2026-08-30T00:00:00.000Z');
  liveWithLaunchTime=@(& $ids $live '2026-08-30T00:00:00.000Z');
  reusedRoot=@(& $ids $live '2026-08-29T23:00:00.000Z')
}|ConvertTo-Json -Compress
`);
  assert.deepEqual(observed, {
    exitedWithoutLaunchTime: [],
    exitedWithLaunchTime: [202],
    liveWithLaunchTime: [101, 202],
    reusedRoot: [],
  });
});

test("a PID that stops naming the identified process before termination is never killed", () => {
  const observed = runPowerShellFixture(`${commonFixture}
$script:now=[DateTime]::Parse('2026-08-30T00:00:00Z');$script:stops=@();
$snapshot={return @($rootRow)};
$stop={param($identity)$script:stops+=[int]$identity.processId;$false};
$clock={return $script:now};$delay={param([int]$milliseconds)$script:now=$script:now.AddSeconds(6)};
$result=Invoke-TestFootprintTree $root $store $snapshot $stop $clock $delay;
[ordered]@{forced=$result.forced;remaining=@($result.remaining);reused=@($result.reusedProcessIds);stops=@($script:stops)}|ConvertTo-Json -Compress
`);
  assert.deepEqual(observed, {
    forced: false,
    remaining: [101],
    reused: [101],
    stops: [101],
  });
});

test("exact process termination validates and kills through one retained handle", () => {
  const observed = runPowerShellFixture(String.raw`
$powershell=(Get-Process -Id $PID).Path;
$child=Start-Process -FilePath $powershell -ArgumentList @('-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 30') -WindowStyle Hidden -PassThru;
try{
  $identity=Get-FootprintLaunchIdentity $child $powershell;
  $other=[ordered]@{processId=$identity.processId;executablePath=$identity.executablePath;creationDate=[DateTime]::Parse($identity.creationDate).ToUniversalTime().AddSeconds(-1).ToString("yyyy-MM-ddTHH:mm:ss.fff'Z'")};
  $refused=Stop-FootprintExactProcess $other;
  $child.Refresh();$aliveAfterRefusal=-not $child.HasExited;
  $stopped=Stop-FootprintExactProcess $identity;
  $exited=$child.WaitForExit(5000);
  [ordered]@{refused=$refused;aliveAfterRefusal=$aliveAfterRefusal;stopped=$stopped;exited=$exited}|ConvertTo-Json -Compress
}finally{
  $child.Refresh();if(-not $child.HasExited){$child.Kill();[void]$child.WaitForExit(5000)}
}
`);
  assert.deepEqual(observed, {
    refused: false,
    aliveAfterRefusal: true,
    stopped: true,
    exited: true,
  });
});

test("a launch identity is captured before the process has initialised its main module", () => {
  const observed = runPowerShellFixture(String.raw`
$powershell=(Get-Process -Id $PID).Path;
$cmd=Join-Path $env:SystemRoot 'System32\cmd.exe';
$startup=([wmiclass]'Win32_ProcessStartup').CreateInstance();$startup.CreateFlags=[uint32]4;
$created=([wmiclass]'Win32_Process').Create(('"'+$powershell+'" -NoProfile -NonInteractive -Command exit'),$null,$startup);
if($created.ReturnValue -ne 0){throw "suspended launch failed: $($created.ReturnValue)"};
$suspended=Get-Process -Id ([int]$created.ProcessId);
$running=Start-Process -FilePath $powershell -ArgumentList @('-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 30') -WindowStyle Hidden -PassThru;
try{
  $mainModuleReadable=$null -ne $suspended.Path;
  $identity=Get-FootprintLaunchIdentity $suspended $powershell;
  $exact=Open-FootprintExactProcess $identity;$creationMatches=$null -ne $exact;if($creationMatches){$exact.Dispose()};
  $suspendedWrong=$null;try{[void](Get-FootprintLaunchIdentity $suspended $cmd)}catch{$suspendedWrong=$_.Exception.Message};
  $runningWrong=$null;try{[void](Get-FootprintLaunchIdentity $running $cmd)}catch{$runningWrong=$_.Exception.Message};
  [ordered]@{mainModuleReadable=$mainModuleReadable;processIdMatches=($identity.processId -eq $suspended.Id);executablePathMatches=($identity.executablePath -eq [IO.Path]::GetFullPath($powershell));creationMatches=$creationMatches;suspendedWrong=$suspendedWrong;runningWrong=$runningWrong}|ConvertTo-Json -Compress
}finally{
  foreach($child in @($suspended,$running)){Stop-Process -Id $child.Id -Force -ErrorAction SilentlyContinue;[void]$child.WaitForExit(5000)}
}
`);
  assert.deepEqual(observed, {
    mainModuleReadable: false,
    processIdMatches: true,
    executablePathMatches: true,
    creationMatches: true,
    suspendedWrong:
      "The launched process object does not identify the exact executable.",
    runningWrong:
      "The launched process object does not identify the exact executable.",
  });
});

test("a launch identity captured immediately holds with every core busy at BelowNormal priority", () => {
  const load = Array.from({ length: os.availableParallelism() }, () => {
    const busy = spawn(process.execPath, ["-e", "for(;;){}"], {
      stdio: "ignore",
      windowsHide: true,
    });
    os.setPriority(busy.pid, os.constants.priority.PRIORITY_BELOW_NORMAL);
    return busy;
  });
  try {
    const observed = runPowerShellFixture(
      String.raw`
$powershell=(Get-Process -Id $PID).Path;$launches=20;$captured=0;$failures=@();
for($i=0;$i -lt $launches;$i++){
  $child=Start-Process -FilePath $powershell -ArgumentList @('-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 30') -WindowStyle Hidden -PassThru;
  try{
    $identity=Get-FootprintLaunchIdentity $child $powershell;
    if($identity.processId -eq $child.Id -and $identity.executablePath -eq [IO.Path]::GetFullPath($powershell)){$captured++}else{$failures+='mismatched identity'}
  }catch{$failures+=$_.Exception.Message}finally{Stop-Process -Id $child.Id -Force -ErrorAction SilentlyContinue;[void]$child.WaitForExit(5000)}
};
[ordered]@{launches=$launches;captured=$captured;failures=@($failures)}|ConvertTo-Json -Compress
`,
      { timeout: 120_000 },
    );
    assert.deepEqual(observed, { launches: 20, captured: 20, failures: [] });
  } finally {
    for (const busy of load) busy.kill();
  }
});
