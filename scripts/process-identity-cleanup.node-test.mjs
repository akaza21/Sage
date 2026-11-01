import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
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

function runPowerShellFixture(body) {
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
      timeout: 10_000,
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
function Invoke-TestFootprintTree($rootIdentity,$knownIdentities,$snapshotProvider,$stopProcessById,$utcNow,$delay,$graceSeconds=1){
  $module=Get-Module ProcessIdentity;
  return & $module {
    param($rootValue,$knownValue,$snapshotValue,$stopValue,$clockValue,$delayValue,$graceValue)
    Invoke-FootprintTreeCore $rootValue $knownValue $graceValue $snapshotValue $stopValue $clockValue $delayValue
  } $rootIdentity $knownIdentities $snapshotProvider $stopProcessById $utcNow $delay $graceSeconds
};
`;

test("zero-grace teardown reaches authenticated zero before a later PID reuse", () => {
  const observed = runPowerShellFixture(`${commonFixture}
$script:rootAlive=$true;$script:childAlive=$true;$script:elapsed=0;
$started=[DateTime]::Parse('2026-08-30T00:00:00Z');
$snapshot={
  $rows=@();
  if($script:rootAlive){if($script:elapsed-eq0){$rows+=$rootRow}else{$rows+=$reusedRootRow}};
  if($script:childAlive){$rows+=$childRow};
  return $rows
};
$stop={param([int]$processId)if($processId-eq202){$script:childAlive=$false}elseif($processId-eq101){$script:rootAlive=$false}};
$clock={return $started.AddMilliseconds($script:elapsed)};
$delay={param([int]$milliseconds)$script:elapsed+=$milliseconds};
$result=Invoke-TestFootprintTree $root $store $snapshot $stop $clock $delay 0;
[ordered]@{forced=$result.forced;identityLost=$result.identityLost;remaining=@($result.remaining);reused=@($result.reusedProcessIds)}|ConvertTo-Json -Compress
`);
  assert.deepEqual(observed, {
    forced: true,
    identityLost: false,
    remaining: [],
    reused: [],
  });
});

test("teardown ignores unrelated processes while retaining distinct remembered identities for one PID", () => {
  const observed = runPowerShellFixture(`${commonFixture}
[void](Add-FootprintRememberedIdentity $store $childRow);
$newChild=[pscustomobject]@{ProcessId=202;ParentProcessId=101;ExecutablePath=$childRow.ExecutablePath;CreationDate='2026-08-30T00:00:02.000Z'};
[void](Add-FootprintRememberedIdentity $store $newChild);
$noise=@(1000..1999 | ForEach-Object { [pscustomobject]@{ProcessId=$_;ParentProcessId=1;ExecutablePath=$childRow.ExecutablePath;CreationDate='2026-08-30T00:00:03.000Z'} });
$plan=Get-FootprintTeardownPlan $root $store (@($rootRow,$newChild)+$noise);
[ordered]@{rootState=$plan.rootState;identityLost=$plan.identityLost;candidates=@($plan.candidates | ForEach-Object ProcessId);reused=@($plan.reusedProcessIds)}|ConvertTo-Json -Compress
`);
  assert.deepEqual(observed, {
    rootState: "matching",
    identityLost: false,
    candidates: [101, 202],
    reused: [],
  });
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
$stop={param([int]$processId)$script:stops+=$processId;if($processId-eq202){$script:childAlive=$false}elseif($processId-eq101){$script:rootAlive=$false;$script:rootStopped=$true}else{throw 'unexpected process'}};
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

test("root PID reuse before exact root termination remains fatal", () => {
  const observed = runPowerShellFixture(`${commonFixture}
$script:now=[DateTime]::Parse('2026-08-30T00:00:00Z');$script:stops=@();
$snapshot={return @($reusedRootRow)};
$stop={param([int]$processId)$script:stops+=$processId};
$clock={return $script:now};$delay={param([int]$milliseconds)$script:now=$script:now.AddSeconds(2)};
$result=Invoke-TestFootprintTree $root $store $snapshot $stop $clock $delay;
[ordered]@{identityLost=$result.identityLost;remaining=@($result.remaining);reused=@($result.reusedProcessIds);stops=@($script:stops)}|ConvertTo-Json -Compress
`);
  assert.deepEqual(observed, {
    identityLost: true,
    remaining: [],
    reused: [101],
    stops: [],
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
$stop={param([int]$processId)$script:stops+=$processId;if($processId-eq101){$script:rootAlive=$false;$script:rootStopped=$true}};
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
$stop={param([int]$processId)$script:stops+=$processId;throw 'injected stop failure'};
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
$stop={param([int]$processId)$script:stops+=$processId;$script:rootAlive=$false;throw 'process not found'};
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
