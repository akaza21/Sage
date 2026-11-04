$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

Import-Module (Join-Path $PSScriptRoot "footprint/C0BObservation.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "footprint/WindowsFootprint.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "footprint/ProcessIdentity.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "footprint/WindowsJob.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "footprint/DisposableProfile.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "footprint/SemanticUia.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "footprint/ScenarioEvidence.psm1") -Force

$script:CheckCount = 0
function Assert-Equal {
  param(
    [Parameter(Mandatory)]$Expected,
    [Parameter(Mandatory)]$Actual,
    [Parameter(Mandatory)][string]$Message
  )
  $script:CheckCount += 1
  if ($Expected -ne $Actual) { throw "$Message Expected '$Expected', got '$Actual'." }
}

function Assert-True {
  param([Parameter(Mandatory)][bool]$Condition, [Parameter(Mandatory)][string]$Message)
  $script:CheckCount += 1
  if (-not $Condition) { throw $Message }
}

function Assert-Throws {
  param([Parameter(Mandatory)][scriptblock]$Action, [Parameter(Mandatory)][string]$Message)
  $script:CheckCount += 1
  try { & $Action } catch { return }
  throw $Message
}

$profileTestRoot = Join-Path ([IO.Path]::GetTempPath()) ("sage-c0b-profile-test-{0}" -f [Guid]::NewGuid().ToString("N"))
[void][IO.Directory]::CreateDirectory($profileTestRoot)
try {
  $profile = New-C0BDisposableProfile $profileTestRoot "epoch-0001"
  Assert-True ([IO.Path]::IsPathRooted($profile.root)) "Disposable profile did not return an absolute root."
  Assert-True ($profile.identitySha256 -match '^[0-9a-f]{64}$') "Disposable profile identity is invalid."
  Assert-Equal "sage-disposable-profile-v1`n" ([Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes((Join-Path $profile.root ".sage-disposable-profile-v1")))) "Disposable profile marker bytes changed."
  Assert-Throws { New-C0BDisposableProfile $profileTestRoot "epoch-0001" } "A disposable profile identity was reused."
  Assert-True (Test-Path -LiteralPath $profile.root -PathType Container) "A duplicate creator deleted the owned profile."
  $blockedClaim = Join-Path $profileTestRoot ".epoch-0002.sage-profile-claim"
  [IO.File]::WriteAllBytes($blockedClaim, [byte[]]@())
  Assert-Throws { New-C0BDisposableProfile $profileTestRoot "epoch-0002" } "A claimed profile identity was reused."
  Assert-True (-not (Test-Path -LiteralPath (Join-Path $profileTestRoot "epoch-0002"))) "A failed claimant created or deleted another profile."
  Remove-Item -LiteralPath $blockedClaim -Force
  Remove-C0BDisposableProfile $profileTestRoot $profile.root
  Assert-True (-not (Test-Path -LiteralPath $profile.root)) "A marked disposable profile was not removed."
} finally {
  Remove-Item -LiteralPath $profileTestRoot -Recurse -Force
}

Assert-Equal 2.5 (Get-FootprintMedian @(4.0, 1.0)) "Even median failed."
Assert-Throws { Get-C0BSemanticWindow ([IntPtr]::Zero) $PID "Sage" } "Semantic UIA accepted a zero window handle."
Assert-Equal 3.0 (Get-FootprintMedian @(3.0, 1.0, 9.0)) "Odd median failed."
Assert-Equal 5.0 (Get-FootprintPercentile @(1.0, 2.0, 3.0, 4.0, 5.0) 0.95) "Nearest-rank P95 failed."
Assert-Equal 0.5 (Get-FootprintCpuPercent 10.0 10.02 4.0) "Raw CPU arithmetic failed."
$coreIdentity = "1`0c:\apps\sage.exe`02026-08-01T00:00:00.000Z"
$measuredCpu = Get-FootprintCpuMeasurement `
  ([ordered]@{ coreCpuSeconds = 10.0; sampleElapsedSeconds = 0.25; coreProcessIdentityKeys = @($coreIdentity); processes = @([pscustomobject]@{ identityKey = $coreIdentity; cpuSeconds = 10.0; coreIncluded = $true }) }) `
  ([ordered]@{ coreCpuSeconds = 10.02; sampleElapsedSeconds = 4.75; coreProcessIdentityKeys = @($coreIdentity); processes = @([pscustomobject]@{ identityKey = $coreIdentity; cpuSeconds = 10.02; coreIncluded = $true }) })
Assert-Equal 4.5 $measuredCpu.measuredWallSeconds "CPU measurement did not retain measured wall duration."
Assert-Equal 0.444444 $measuredCpu.percentOneCore "CPU measurement used nominal rather than measured duration."
Assert-Throws {
  Get-FootprintCpuMeasurement `
    ([ordered]@{ coreCpuSeconds = 10.0; sampleElapsedSeconds = 1.0; coreProcessIdentityKeys = @($coreIdentity); processes = @([pscustomobject]@{ identityKey = $coreIdentity; cpuSeconds = 10.0; coreIncluded = $true }) }) `
    ([ordered]@{ coreCpuSeconds = 10.1; sampleElapsedSeconds = 1.0; coreProcessIdentityKeys = @($coreIdentity); processes = @([pscustomobject]@{ identityKey = $coreIdentity; cpuSeconds = 10.1; coreIncluded = $true }) })
} "A zero-duration CPU sample window passed."
Assert-Throws { Get-FootprintCpuPercent 10.1 10.0 1.0 } "A negative aggregate CPU delta was clamped instead of rejected."
$replacementIdentity = "2`0c:\apps\renderer.exe`02026-08-01T00:00:01.000Z"
Assert-Throws {
  Get-FootprintCpuMeasurement `
    ([ordered]@{ coreCpuSeconds = 10.0; sampleElapsedSeconds = 1.0; coreProcessIdentityKeys = @($coreIdentity); processes = @([pscustomobject]@{ identityKey = $coreIdentity; cpuSeconds = 10.0; coreIncluded = $true }) }) `
    ([ordered]@{ coreCpuSeconds = 10.1; sampleElapsedSeconds = 2.0; coreProcessIdentityKeys = @($replacementIdentity); processes = @([pscustomobject]@{ identityKey = $replacementIdentity; cpuSeconds = 10.1; coreIncluded = $true }) })
} "Core process churn did not invalidate CPU measurement."
Assert-Throws {
  Get-FootprintCpuMeasurement `
    ([ordered]@{ coreCpuSeconds = 10.0; sampleElapsedSeconds = 1.0; coreProcessIdentityKeys = @($coreIdentity); processes = @([pscustomobject]@{ identityKey = $coreIdentity; cpuSeconds = 10.0; coreIncluded = $true }) }) `
    ([ordered]@{ coreCpuSeconds = 9.9; sampleElapsedSeconds = 2.0; coreProcessIdentityKeys = @($coreIdentity); processes = @([pscustomobject]@{ identityKey = $coreIdentity; cpuSeconds = 9.9; coreIncluded = $true }) })
} "A negative per-identity CPU delta passed."
$stableFirst = [ordered]@{ coreCpuSeconds = 10.0; sampleElapsedSeconds = 1.0; coreProcessIdentityKeys = @($coreIdentity); processes = @([pscustomobject]@{ identityKey = $coreIdentity; cpuSeconds = 10.0; coreIncluded = $true }) }
$stableLast = [ordered]@{ coreCpuSeconds = 10.2; sampleElapsedSeconds = 3.0; coreProcessIdentityKeys = @($coreIdentity); processes = @([pscustomobject]@{ identityKey = $coreIdentity; cpuSeconds = 10.2; coreIncluded = $true }) }
$churnMiddle = [ordered]@{ coreCpuSeconds = 10.1; sampleElapsedSeconds = 2.0; coreProcessIdentityKeys = @($replacementIdentity); processes = @([pscustomobject]@{ identityKey = $replacementIdentity; cpuSeconds = 10.1; coreIncluded = $true }) }
Assert-Throws { Assert-FootprintStableCpuSamples @($stableFirst, $churnMiddle, $stableLast) } "Intermediate process churn did not invalidate the launch."
$negativeMiddle = [ordered]@{ coreCpuSeconds = 9.9; sampleElapsedSeconds = 2.0; coreProcessIdentityKeys = @($coreIdentity); processes = @([pscustomobject]@{ identityKey = $coreIdentity; cpuSeconds = 9.9; coreIncluded = $true }) }
Assert-Throws { Assert-FootprintStableCpuSamples @($stableFirst, $negativeMiddle, $stableLast) } "An intermediate negative CPU delta did not invalidate the launch."
Assert-True (Assert-FootprintStableCpuSamples @($stableFirst, $stableLast)) "Stable canonical process membership was rejected."
$currentProcess = [Diagnostics.Process]::GetCurrentProcess()
$currentExecutableSha256 = (Get-FileHash -LiteralPath $currentProcess.Path -Algorithm SHA256).Hash.ToLowerInvariant()
$nativeIdentity = Get-FootprintNativeProcessIdentity $currentProcess $currentProcess.Path $currentExecutableSha256
Assert-Equal $currentProcess.Id $nativeIdentity.pid "Native process identity changed the launched PID."
Assert-Equal ($currentProcess.StartTime.ToUniversalTime().ToFileTimeUtc().ToString()) $nativeIdentity.creationTimeFiletime100ns "Native process identity lost FILETIME precision."
Assert-True ($nativeIdentity.processIdentitySha256 -match '^[0-9a-f]{64}$') "Native process identity digest is invalid."
$sanitizedTree = Get-C0BSanitizedTreeSnapshot $currentProcess.Id
Assert-Equal "membershipSha256,coreProcessCount,unknownProcessCount,coreCpuTime100ns,corePrivateBytes,coreWorkingSetBytes" (($sanitizedTree.Keys) -join ",") "C0-B tree snapshot exposed uncontracted process details."
Assert-True ($sanitizedTree.membershipSha256 -match '^[0-9a-f]{64}$') "C0-B tree membership digest is invalid."
$monotonic = Get-FootprintMonotonicClock
$cadenceBaseline = $monotonic.tick + [long][Math]::Ceiling($monotonic.frequencyHz / 50.0)
$cadence = @(Invoke-FootprintAbsoluteCadence $cadenceBaseline 50 3 { param($ordinal, $dueTick) return "$ordinal`0$dueTick" })
Assert-Equal 3 $cadence.Count "Absolute QPC cadence returned the wrong sample count."
Assert-True ($cadence[0].capturedTick -ge $cadence[0].dueTick) "Absolute QPC cadence captured before its due tick."
Assert-Equal 100000 (Get-FootprintMonotonicDurationUs $cadence[0].dueTick $cadence[2].dueTick $monotonic.frequencyHz) "QPC duration derivation is not exact."
$processLimits = Get-FootprintProcessLimits
Assert-Equal 5 $processLimits.postKillWaitSeconds "Post-kill convergence wait is not mechanically bounded."
$creationBase = [DateTime]::Parse("2026-08-01T09:11:07.8910000Z").ToUniversalTime()
$creationRow = [pscustomobject]@{ ProcessId = 1; ExecutablePath = "C:\Apps\sage.exe"; CreationDate = $creationBase }
$driftedCreationRow = [pscustomobject]@{ ProcessId = 1; ExecutablePath = "C:\Apps\sage.exe"; CreationDate = $creationBase.AddTicks(9) }
$reusedCreationRow = [pscustomobject]@{ ProcessId = 1; ExecutablePath = "C:\Apps\sage.exe"; CreationDate = $creationBase.AddMilliseconds(1) }
Assert-Equal (ConvertTo-FootprintProcessIdentity $creationRow).creationDate (ConvertTo-FootprintProcessIdentity $driftedCreationRow).creationDate "Sub-millisecond CIM creation drift changed process identity."
Assert-True ((ConvertTo-FootprintProcessIdentity $creationRow).creationDate -ne (ConvertTo-FootprintProcessIdentity $reusedCreationRow).creationDate) "A one-millisecond creation change did not change process identity."

$roles = @(
  @("native-host", "sage.exe", $null, $true),
  @("browser", "msedgewebview2.exe", "msedgewebview2.exe --embedded-browser-webview=1", $false),
  @("renderer", "msedgewebview2.exe", "msedgewebview2.exe --type=renderer", $false),
  @("gpu", "msedgewebview2.exe", "msedgewebview2.exe --type=gpu-process", $false),
  @("utility-network", "msedgewebview2.exe", "msedgewebview2.exe --type=utility --utility-sub-type=network.mojom.NetworkService", $false),
  @("utility-storage", "msedgewebview2.exe", "msedgewebview2.exe --type=utility --utility-sub-type=storage.mojom.StorageService", $false),
  @("crashpad", "msedgewebview2.exe", "msedgewebview2.exe --type=crashpad-handler", $false),
  @("excluded-child", "pwsh.exe", "pwsh.exe", $false),
  @("unknown", "surprise.exe", "surprise.exe", $false)
)
foreach ($case in $roles) {
  Assert-Equal $case[0] (Get-FootprintProcessRole $case[1] $case[2] $case[3]) "Process role classification failed."
}

$treeStart = [DateTime]::Parse("2026-07-30T00:00:00Z").ToUniversalTime()
$rows = @(
  [pscustomobject]@{ ProcessId = 10; ParentProcessId = 1; CreationDate = $treeStart },
  [pscustomobject]@{ ProcessId = 11; ParentProcessId = 10; CreationDate = $treeStart.AddSeconds(1) },
  [pscustomobject]@{ ProcessId = 12; ParentProcessId = 11; CreationDate = $treeStart.AddSeconds(2) },
  [pscustomobject]@{ ProcessId = 20; ParentProcessId = 1; CreationDate = $treeStart.AddSeconds(3) }
)
$descendants = @(Get-FootprintDescendants 10 $rows)
Assert-Equal 3 $descendants.Count "Descendant count failed."
Assert-Equal "10,11,12" (($descendants | ForEach-Object ProcessId) -join ",") "Descendant membership failed."
Assert-Throws { Get-FootprintDescendants 10 $rows 2 128 } "An over-limit system process snapshot was collected."
Assert-Throws { Get-FootprintDescendants 10 $rows 4096 2 } "An over-limit candidate descendant set was retained."

$staleRows = @(
  [pscustomobject]@{ ProcessId = 10; ParentProcessId = 1; CreationDate = $treeStart },
  [pscustomobject]@{ ProcessId = 11; ParentProcessId = 10; CreationDate = $treeStart.AddSeconds(-1) },
  [pscustomobject]@{ ProcessId = 12; ParentProcessId = 10; CreationDate = $null },
  [pscustomobject]@{ ProcessId = 13; ParentProcessId = 12; CreationDate = $treeStart.AddSeconds(1) }
)
Assert-Equal "10,12" ((@(Get-FootprintDescendants 10 $staleRows) | ForEach-Object ProcessId) -join ",") "An older or unprovable process was adopted into the tree."
$unknownRootRows = @(
  [pscustomobject]@{ ProcessId = 10; ParentProcessId = 1; CreationDate = $null },
  [pscustomobject]@{ ProcessId = 11; ParentProcessId = 10; CreationDate = $treeStart }
)
Assert-Equal "10" ((@(Get-FootprintDescendants 10 $unknownRootRows) | ForEach-Object ProcessId) -join ",") "A root without a proven creation time vouched for its children."
$exitedRootRows = @(
  [pscustomobject]@{ ProcessId = 11; ParentProcessId = 10; CreationDate = $treeStart.AddSeconds(-1) },
  [pscustomobject]@{ ProcessId = 12; ParentProcessId = 10; CreationDate = $treeStart.AddSeconds(1) }
)
Assert-Equal "" ((@(Get-FootprintDescendants 10 $exitedRootRows) | ForEach-Object ProcessId) -join ",") "An exited root without a launch time vouched for processes naming its PID."
Assert-Equal "12" ((@(Get-FootprintDescendants 10 $exitedRootRows -RootCreationDate $treeStart) | ForEach-Object ProcessId) -join ",") "An exited root's launch time did not prove its own children."
Assert-Equal 0 @(Get-FootprintDescendants 10 $rows -RootCreationDate $treeStart.AddSeconds(30)).Count "A reused root PID vouched for its tree."

$rootIdentity = [ordered]@{
  processId = 10
  executablePath = "C:\Apps\sage.exe"
  creationDate = "2026-07-30T00:00:00.0000000Z"
}
$teardownRows = @(
  [pscustomobject]@{ ProcessId = 10; ParentProcessId = 1; ExecutablePath = "C:\Apps\sage.exe"; CreationDate = "2026-07-30T00:00:00.0000000Z" },
  [pscustomobject]@{ ProcessId = 11; ParentProcessId = 10; ExecutablePath = "C:\Apps\msedgewebview2.exe"; CreationDate = "2026-07-30T00:00:01.0000000Z" },
  [pscustomobject]@{ ProcessId = 12; ParentProcessId = 1; ExecutablePath = "C:\Windows\unrelated.exe"; CreationDate = "2026-07-30T00:00:09.0000000Z" }
)
$forceCandidates = @(Get-FootprintForceKillCandidates $rootIdentity $teardownRows)
Assert-Equal "10,11" (($forceCandidates | ForEach-Object ProcessId) -join ",") "Reused remembered PID was not excluded."
$reusedRootRows = @(
  [pscustomobject]@{ ProcessId = 10; ParentProcessId = 1; ExecutablePath = "C:\Apps\sage.exe"; CreationDate = "2026-07-30T00:01:00.0000000Z" },
  [pscustomobject]@{ ProcessId = 11; ParentProcessId = 10; ExecutablePath = "C:\Apps\msedgewebview2.exe"; CreationDate = "2026-07-30T00:01:01.0000000Z" }
)
Assert-Equal 0 @(Get-FootprintForceKillCandidates $rootIdentity $reusedRootRows).Count "Reused root PID was not rejected."
Assert-Throws {
  Get-FootprintRootIdentity 10 $rootIdentity.executablePath $rootIdentity.creationDate $reusedRootRows
} "Root identity acquisition accepted a reused creation identity."

$remembered = New-FootprintIdentityStore
foreach ($row in $teardownRows[0..1]) {
  [void](Add-FootprintRememberedIdentity $remembered $row)
}
$rootGoneChildAlive = Get-FootprintTeardownPlan $rootIdentity $remembered @($teardownRows[1])
Assert-Equal "absent" $rootGoneChildAlive.rootState "Root exit was not recognized."
Assert-Equal "11" (($rootGoneChildAlive.candidates | ForEach-Object ProcessId) -join ",") "A remembered orphan child was lost after root exit."

$rootReusedChildAlive = Get-FootprintTeardownPlan $rootIdentity $remembered $reusedRootRows
Assert-Equal "reused" $rootReusedChildAlive.rootState "Root PID reuse was not recognized."
Assert-True (-not $rootReusedChildAlive.identityLost) "A different creation time did not prove the tracked root exited."
Assert-Equal "" (($rootReusedChildAlive.candidates | ForEach-Object ProcessId) -join ",") "A reused root or child identity was authorized for kill."

$matchingRootReusedChild = @(
  $teardownRows[0],
  [pscustomobject]@{ ProcessId = 11; ParentProcessId = 10; ExecutablePath = "C:\Apps\msedgewebview2.exe"; CreationDate = "2026-07-30T00:02:01.0000000Z" }
)
$childReusePlan = Get-FootprintTeardownPlan $rootIdentity $remembered $matchingRootReusedChild
Assert-Equal "10" (($childReusePlan.candidates | ForEach-Object ProcessId) -join ",") "Child PID reuse authorized the wrong process for kill."
Assert-Equal "11" ($childReusePlan.reusedProcessIds -join ",") "Child PID reuse was not retained for audit."

$delayedExitPlan = Get-FootprintTeardownPlan $rootIdentity $remembered @()
Assert-Equal 0 $delayedExitPlan.candidates.Count "A delayed child exit did not converge to a clean plan."

$launchCleanupStore = New-FootprintIdentityStore
[void](Add-FootprintRememberedIdentity $launchCleanupStore $teardownRows[0])
$identityAcquisitionCleanup = Get-FootprintTeardownPlan $rootIdentity $launchCleanupStore @($teardownRows[0])
Assert-Equal "10" (($identityAcquisitionCleanup.candidates | ForEach-Object ProcessId) -join ",") "Identity-acquisition failure lacks an exact cleanup plan."

$script:fakeProcessExited = $false
$script:fakeProcessForced = $false
$fakeProcess = [pscustomobject]@{
  Id = [int]::MaxValue
  MainWindowHandle = [IntPtr]::Zero
  StartTime = [DateTime]::Parse("2026-07-30T00:00:00Z")
}
$fakeProcess | Add-Member -MemberType ScriptProperty -Name HasExited -Value { $script:fakeProcessExited }
$fakeProcess | Add-Member -MemberType ScriptMethod -Name Refresh -Value {}
$fakeProcess | Add-Member -MemberType ScriptMethod -Name CloseMainWindow -Value { return $false }
$fakeProcess | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value {
  param([int]$Milliseconds)
  return $script:fakeProcessExited
}
$fakeProcess | Add-Member -MemberType ScriptMethod -Name Kill -Value {
  param([bool]$EntireProcessTree)
  if (-not $EntireProcessTree) { throw "The fallback did not request process-tree termination." }
  $script:fakeProcessForced = $true
  $script:fakeProcessExited = $true
}
$processObjectCleanup = Stop-FootprintLaunchedProcessObject $fakeProcess "C:\Apps\sage.exe" $null @() 0 100
Assert-True $processObjectCleanup.exited "Exact process-object fallback cleanup did not verify exit."
Assert-True $processObjectCleanup.forced "Exact process-object fallback cleanup did not force the process tree."
Assert-True $processObjectCleanup.treeBoundaryVerified "Exact process-object fallback cleanup did not verify a process-tree boundary."
Assert-True $script:fakeProcessForced "Exact process-object fallback did not use the launched process object's tree-kill method."

$script:fakeRootOnlyExited = $false
$fakeRootOnlyProcess = [pscustomobject]@{
  Id = [int]::MaxValue
  MainWindowHandle = [IntPtr]::Zero
  StartTime = [DateTime]::Parse("2026-07-30T00:00:00Z")
}
$fakeRootOnlyProcess | Add-Member -MemberType ScriptProperty -Name HasExited -Value { $script:fakeRootOnlyExited }
$fakeRootOnlyProcess | Add-Member -MemberType ScriptMethod -Name Refresh -Value {}
$fakeRootOnlyProcess | Add-Member -MemberType ScriptMethod -Name CloseMainWindow -Value { return $false }
$fakeRootOnlyProcess | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value {
  param([int]$Milliseconds)
  return $script:fakeRootOnlyExited
}
$fakeRootOnlyProcess | Add-Member -MemberType ScriptMethod -Name Kill -Value {
  param([AllowNull()][object]$EntireProcessTree)
  if ($EntireProcessTree) { throw "Tree-kill overload unavailable." }
  $script:fakeRootOnlyExited = $true
}
Assert-Throws {
  Stop-FootprintLaunchedProcessObject $fakeRootOnlyProcess "C:\Apps\sage.exe" $null @() 0 100
} "A root-only fallback without an exact descendant snapshot did not fail closed."
Assert-True $script:fakeRootOnlyExited "A failed tree-boundary proof leaked the exact launched root process."
$script:fakeRootOnlyExited = $false
$ps5IdentityStore = New-FootprintIdentityStore
$ps5Rows = @([pscustomobject]@{
  ProcessId = [int]::MaxValue
  ParentProcessId = 1
  ExecutablePath = "C:\Apps\sage.exe"
  CreationDate = [DateTime]::Parse("2026-07-30T00:00:00Z")
})
$ps5Cleanup = Stop-FootprintLaunchedProcessObject $fakeRootOnlyProcess "C:\Apps\sage.exe" $ps5IdentityStore $ps5Rows 0 100
Assert-True $ps5Cleanup.treeBoundaryVerified "PS5 root-only cleanup rejected an exact creation-matched snapshot."
Assert-Equal 1 $ps5IdentityStore.Count "PS5 root-only cleanup did not retain the exact root identity."
Assert-True $script:fakeRootOnlyExited "PS5 root-only cleanup did not close the exact launched Process object."

$script:reusedProcessObjectExited = $false
$script:reusedProcessKillAttempted = $false
$samePathReuseProcess = [pscustomobject]@{
  Id = 10
  MainWindowHandle = [IntPtr]::Zero
  StartTime = [DateTime]::Parse("2026-07-30T00:00:00Z")
}
$samePathReuseProcess | Add-Member -MemberType ScriptProperty -Name HasExited -Value { $script:reusedProcessObjectExited }
$samePathReuseProcess | Add-Member -MemberType ScriptMethod -Name Refresh -Value {}
$samePathReuseProcess | Add-Member -MemberType ScriptMethod -Name CloseMainWindow -Value { return $false }
$samePathReuseProcess | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value {
  param([int]$Milliseconds)
  return $script:reusedProcessObjectExited
}
$samePathReuseProcess | Add-Member -MemberType ScriptMethod -Name Kill -Value {
  param([AllowNull()][object]$EntireProcessTree)
  $script:reusedProcessKillAttempted = $true
  throw "A reused process identity must not be killed."
}
$samePathReuseStore = New-FootprintIdentityStore
Assert-Throws {
  Stop-FootprintLaunchedProcessObject $samePathReuseProcess "C:\Apps\sage.exe" $samePathReuseStore $reusedRootRows 0 100
} "A same-path PID reuse was accepted as the launched process tree."
Assert-Equal 0 $samePathReuseStore.Count "A same-path PID reuse or its descendants were remembered for kill."
Assert-True (-not $script:reusedProcessKillAttempted) "A same-path PID reuse reached a Process-object kill method."
Assert-True (-not $script:reusedProcessObjectExited) "A same-path PID reuse was killed through the launched Process object."

$jobTestRoot = Join-Path ([IO.Path]::GetTempPath()) ("sage-job-test-" + [Guid]::NewGuid().ToString("N"))
[void][IO.Directory]::CreateDirectory($jobTestRoot)
$jobFixture = Join-Path $jobTestRoot "early-exit-fixture.exe"
$jobLaunch = $null
try {
  Add-Type -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Threading;

public static class SageJobEarlyExitFixture {
  public static int Main() {
    if (Environment.GetEnvironmentVariable("SAGE_JOB_TEST_CHILD") == "1") {
      Thread.Sleep(30000);
      return 0;
    }
    string executable = Process.GetCurrentProcess().MainModule.FileName;
    var start = new ProcessStartInfo(executable) { UseShellExecute = false };
    start.EnvironmentVariables["SAGE_JOB_TEST_CHILD"] = "1";
    using (Process child = Process.Start(start)) { return child == null ? 1 : 0; }
  }
}
'@ -OutputAssembly $jobFixture -OutputType ConsoleApplication
  $jobStart = [Diagnostics.ProcessStartInfo]::new()
  $jobStart.FileName = $jobFixture
  $jobStart.WorkingDirectory = $jobTestRoot
  $jobStart.UseShellExecute = $false
  $jobStart.EnvironmentVariables["SAGE_JOB_TEST_CHILD"] = "0"
  $jobLaunch = Start-FootprintJobProcess $jobStart 2
  Assert-True ($jobLaunch.process.WaitForExit(5000)) "The Job test root did not exit."
  Start-Sleep -Milliseconds 100
  Assert-Equal 1 $jobLaunch.boundary.ActiveProcesses() "The early-exit child escaped the native Job."
  $jobCleanup = Stop-FootprintJobProcess $jobLaunch.process $jobLaunch.boundary 0 200
  Assert-True $jobCleanup.forced "The early-exit child did not require authoritative Job termination."
  Assert-True $jobCleanup.treeBoundaryVerified "The native Job cleanup did not verify its tree boundary."
  Assert-Equal 0 $jobCleanup.orphanProcessesAfterClose "The native Job retained an orphan."
  Assert-True (
    $jobCleanup.orphanCheckCompletedTick - $jobCleanup.closeRequestedTick -ge
      [long][Math]::Ceiling(([double][Diagnostics.Stopwatch]::Frequency * 200) / 1000.0)
  ) "The native Job did not remain empty through the complete orphan window."
} finally {
  if ($null -ne $jobLaunch) {
    try { $jobLaunch.boundary.Terminate() } catch {}
    $jobLaunch.boundary.Dispose()
    $jobLaunch.process.Dispose()
  }
  Remove-Item -LiteralPath $jobTestRoot -Recurse -Force -ErrorAction SilentlyContinue
}

$recordBoundStore = New-FootprintIdentityStore
for ($index = 0; $index -lt 128; $index += 1) {
  [void](Add-FootprintRememberedIdentity $recordBoundStore ([pscustomobject]@{
    ProcessId = 1000 + $index
    ExecutablePath = "C:\Apps\child.exe"
    CreationDate = "2026-07-30T00:00:$($index.ToString('D2')).0000000Z"
  }))
}
Assert-Throws {
  Add-FootprintRememberedIdentity $recordBoundStore ([pscustomobject]@{
    ProcessId = 9999; ExecutablePath = "C:\Apps\overflow.exe"; CreationDate = "2026-07-30T01:00:00.0000000Z"
  })
} "The remembered process record cap was not enforced."

$byteBoundStore = New-FootprintIdentityStore
Assert-Throws {
  for ($index = 0; $index -lt 100; $index += 1) {
    [void](Add-FootprintRememberedIdentity $byteBoundStore ([pscustomobject]@{
      ProcessId = 2000 + $index
      ExecutablePath = "C:\Apps\$(('x' * 900))-$index.exe"
      CreationDate = "2026-07-30T00:01:$($index.ToString('D2')).0000000Z"
    }))
  }
} "The remembered process identity-byte cap was not enforced."

$expectedPath = "C:\Apps\sage.exe"
$expectedExecutableHash = "a" * 64
$expectedSourceHash = "b" * 64
$expectedDiagnosticHash = "c" * 64
$expectedFixtureHash = "d" * 64
$setup = [ordered]@{
  scenario = "chat"; executed = $true; semanticVerified = $true; processId = 10
  title = "Sage"; origin = "http://tauri.localhost/"; executablePath = $expectedPath
  executableSha256 = $expectedExecutableHash; sourceAggregateSha256 = $expectedSourceHash
  diagnosticDataDirectorySha256 = $expectedDiagnosticHash
  fixtureProjectRootSha256 = $expectedFixtureHash; projectChatCount = 1
  chatCenterCount = 1; explorerSidebarCount = 0; explorerHeavyViewMounted = $false
}
$chatMeasured = [ordered]@{
  semanticVerified = $true; processId = 20; rootTitle = "Sage"; uiaView = "control"; transitionUiaView = "control"
  chatCenterCount = 1; explorerSidebarCount = 0; explorerHeavyViewMounted = $false
  transitionAction = "invoke-hide-right-sidebar"; transitionInvokeCount = 1; transitionShowControlExposed = $false
}
$explorerMeasured = [ordered]@{
  semanticVerified = $true; processId = 21; rootTitle = "Sage"; uiaView = "control"; transitionUiaView = "control"
  chatCenterCount = 1; explorerSidebarCount = 1; explorerHeavyViewMounted = $true
  transitionAction = "verify-explorer"; transitionInvokeCount = 0; transitionShowControlExposed = $false
}
$cleanSamples = @([ordered]@{ unknownProcessCount = 0; processes = @() })
Assert-True (Assert-FootprintScenarioAcceptance "chat" $setup $chatMeasured $cleanSamples $expectedPath $expectedExecutableHash $expectedSourceHash $expectedDiagnosticHash $expectedFixtureHash) "Chat scenario contract failed."
$closedChatMeasured = [ordered]@{} + $chatMeasured
$closedChatMeasured.transitionAction = "already-closed-noop"
$closedChatMeasured.transitionInvokeCount = 0
$closedChatMeasured.transitionShowControlExposed = $true
Assert-True (Assert-FootprintScenarioAcceptance "chat" $setup $closedChatMeasured $cleanSamples $expectedPath $expectedExecutableHash $expectedSourceHash $expectedDiagnosticHash $expectedFixtureHash) "Persisted closed Chat scenario contract failed."
$providerUnexposedChat = [ordered]@{} + $chatMeasured
$providerUnexposedChat.transitionAction = "already-closed-provider-unexposed"
$providerUnexposedChat.transitionInvokeCount = 0
$providerUnexposedChat.transitionShowControlExposed = $false
Assert-True (Assert-FootprintScenarioAcceptance "chat" $setup $providerUnexposedChat $cleanSamples $expectedPath $expectedExecutableHash $expectedSourceHash $expectedDiagnosticHash $expectedFixtureHash) "Provider-unexposed closed Chat scenario contract failed."
$rawViewChat = [ordered]@{} + $providerUnexposedChat
$rawViewChat.uiaView = "raw"
$rawViewChat.transitionUiaView = "raw"
Assert-True (Assert-FootprintScenarioAcceptance "chat" $setup $rawViewChat $cleanSamples $expectedPath $expectedExecutableHash $expectedSourceHash $expectedDiagnosticHash $expectedFixtureHash) "Exact RawView Chat evidence was rejected."
$unsupportedViewChat = [ordered]@{} + $rawViewChat
$unsupportedViewChat.uiaView = "combined"
Assert-Throws {
  Assert-FootprintScenarioAcceptance "chat" $setup $unsupportedViewChat $cleanSamples $expectedPath $expectedExecutableHash $expectedSourceHash $expectedDiagnosticHash $expectedFixtureHash
} "Combined UIA view evidence passed."
$inconsistentChatTransition = [ordered]@{} + $closedChatMeasured
$inconsistentChatTransition.transitionInvokeCount = 1
Assert-Throws {
  Assert-FootprintScenarioAcceptance "chat" $setup $inconsistentChatTransition $cleanSamples $expectedPath $expectedExecutableHash $expectedSourceHash $expectedDiagnosticHash $expectedFixtureHash
} "A no-op Chat transition recorded an invocation."
$explorerSetup = [ordered]@{
  scenario = "explorer"; executed = $true; semanticVerified = $true; processId = 10
  title = "Sage"; origin = "http://tauri.localhost/"; executablePath = $expectedPath
  executableSha256 = $expectedExecutableHash; sourceAggregateSha256 = $expectedSourceHash
  diagnosticDataDirectorySha256 = $expectedDiagnosticHash
  fixtureProjectRootSha256 = $expectedFixtureHash; projectChatCount = 1
  chatCenterCount = 1; explorerSidebarCount = 1; explorerHeavyViewMounted = $true
}
Assert-True (Assert-FootprintScenarioAcceptance "explorer" $explorerSetup $explorerMeasured $cleanSamples $expectedPath $expectedExecutableHash $expectedSourceHash $expectedDiagnosticHash $expectedFixtureHash) "Explorer scenario contract failed."
$multipleProjectChats = [ordered]@{} + $setup
$multipleProjectChats.projectChatCount = 2
Assert-Throws {
  Assert-FootprintScenarioAcceptance "chat" $multipleProjectChats $chatMeasured $cleanSamples $expectedPath $expectedExecutableHash $expectedSourceHash $expectedDiagnosticHash $expectedFixtureHash
} "Setup with multiple project Chats passed."
Assert-Throws {
  Assert-FootprintScenarioAcceptance "chat" ([ordered]@{ scenario = "chat" }) $chatMeasured $cleanSamples $expectedPath $expectedExecutableHash $expectedSourceHash $expectedDiagnosticHash $expectedFixtureHash
} "A scenario label without setup evidence passed."
$sameProcess = [ordered]@{
  semanticVerified = $true; processId = 10; rootTitle = "Sage"; uiaView = "control"; transitionUiaView = "control"
  chatCenterCount = 1; explorerSidebarCount = 0; explorerHeavyViewMounted = $false
  transitionAction = "invoke-hide-right-sidebar"; transitionInvokeCount = 1; transitionShowControlExposed = $false
}
Assert-Throws {
  Assert-FootprintScenarioAcceptance "chat" $setup $sameProcess $cleanSamples $expectedPath $expectedExecutableHash $expectedSourceHash $expectedDiagnosticHash $expectedFixtureHash
} "Setup and measured launches were not required to be distinct."
$ambiguousExplorer = [ordered]@{
  semanticVerified = $true; processId = 21; rootTitle = "Sage"; uiaView = "control"; transitionUiaView = "control"
  chatCenterCount = 1; explorerSidebarCount = 2; explorerHeavyViewMounted = $true
  transitionAction = "verify-explorer"; transitionInvokeCount = 0; transitionShowControlExposed = $false
}
Assert-Throws {
  Assert-FootprintScenarioAcceptance "explorer" $explorerSetup $ambiguousExplorer $cleanSamples $expectedPath $expectedExecutableHash $expectedSourceHash $expectedDiagnosticHash $expectedFixtureHash
} "An ambiguous Explorer accessibility tree passed."
$chatWithExplorerMounted = [ordered]@{} + $chatMeasured
$chatWithExplorerMounted.explorerSidebarCount = 1
$chatWithExplorerMounted.explorerHeavyViewMounted = $true
Assert-Throws {
  Assert-FootprintScenarioAcceptance "chat" $setup $chatWithExplorerMounted $cleanSamples $expectedPath $expectedExecutableHash $expectedSourceHash $expectedDiagnosticHash $expectedFixtureHash
} "Chat acceptance retained the Explorer heavy view."
$explorerWithoutHeavyView = [ordered]@{} + $explorerMeasured
$explorerWithoutHeavyView.explorerSidebarCount = 0
$explorerWithoutHeavyView.explorerHeavyViewMounted = $false
Assert-Throws {
  Assert-FootprintScenarioAcceptance "explorer" $explorerSetup $explorerWithoutHeavyView $cleanSamples $expectedPath $expectedExecutableHash $expectedSourceHash $expectedDiagnosticHash $expectedFixtureHash
} "Explorer acceptance passed without its heavy view."
$unknownSamples = @([ordered]@{ unknownProcessCount = 1; processes = @() })
Assert-Throws {
  Assert-FootprintScenarioAcceptance "chat" $setup $chatMeasured $unknownSamples $expectedPath $expectedExecutableHash $expectedSourceHash $expectedDiagnosticHash $expectedFixtureHash
} "An unknown descendant did not invalidate acceptance."
$debugSamples = @([ordered]@{
  unknownProcessCount = 0
  processes = @([ordered]@{ commandLine = "msedgewebview2.exe --remote-debugging-port=9222" })
})
Assert-Throws {
  Assert-FootprintScenarioAcceptance "chat" $setup $chatMeasured $debugSamples $expectedPath $expectedExecutableHash $expectedSourceHash $expectedDiagnosticHash $expectedFixtureHash
} "A measured debug descendant did not invalidate acceptance."

foreach ($scenario in @("chat", "explorer")) {
  $contract = Get-FootprintScenarioContract $scenario
  Assert-True ($contract.setupRequired -and $contract.setupVerificationRequired -and $contract.measuredVerificationRequired) "$scenario omitted a required phase."
}
Assert-Equal 0 (Get-FootprintScenarioContract "chat").explorerSidebarCount "Chat contract does not require Explorer to unmount."
Assert-Equal 1 (Get-FootprintScenarioContract "explorer").explorerSidebarCount "Explorer contract does not require its heavy view."
$openChatDecision = Get-FootprintChatTransitionDecision 1 1 1 0
Assert-Equal "invoke-hide-right-sidebar" $openChatDecision.action "An open Chat launch did not select the semantic hide transition."
Assert-Equal 1 $openChatDecision.invokeCount "An open Chat launch did not require exactly one invocation."
$closedChatDecision = Get-FootprintChatTransitionDecision 1 0 0 1
Assert-Equal "already-closed-noop" $closedChatDecision.action "A persisted closed Chat launch did not select the deterministic no-op."
Assert-Equal 0 $closedChatDecision.invokeCount "A persisted closed Chat launch attempted a redundant invocation."
Assert-Equal "already-closed-provider-unexposed" (Get-FootprintChatTransitionDecision 1 0 0 0).action "Provider-unexposed closed Chat state was rejected."
Assert-Throws { Get-FootprintChatTransitionDecision 0 0 0 0 } "Chat-zero unexposed state passed."
Assert-Throws { Get-FootprintChatTransitionDecision 1 1 2 0 } "An ambiguous new Chat transition passed."
Assert-Throws { Get-FootprintChatTransitionDecision 1 0 0 2 } "Multiple show-sidebar controls passed as already closed."
Assert-Throws { Get-FootprintChatTransitionDecision 1 1 1 1 } "Conflicting show/hide sidebar controls passed."
$rawSelectionProbe = [pscustomobject]@{ calls = 0 }
$rawSnapshot = [ordered]@{ uiaView = "raw"; chatCenterCount = 1 }
$selectedRaw = Select-FootprintUiaViewSnapshot ([ordered]@{ uiaView = "control"; chatCenterCount = 0 }) {
  $rawSelectionProbe.calls += 1
  return $rawSnapshot
}
Assert-Equal "raw" $selectedRaw.uiaView "ControlView chat-zero did not select a fresh RawView snapshot."
Assert-Equal 1 $rawSelectionProbe.calls "ControlView chat-zero did not invoke RawView exactly once."
$selectedControl = Select-FootprintUiaViewSnapshot ([ordered]@{ uiaView = "control"; chatCenterCount = 1 }) {
  $rawSelectionProbe.calls += 1
  return $rawSnapshot
}
Assert-Equal "control" $selectedControl.uiaView "ControlView exact Chat did not remain authoritative."
Assert-Equal 1 $rawSelectionProbe.calls "ControlView exact Chat unnecessarily traversed RawView."
$ambiguousControl = Select-FootprintUiaViewSnapshot ([ordered]@{ uiaView = "control"; chatCenterCount = 2 }) {
  $rawSelectionProbe.calls += 1
  return $rawSnapshot
}
Assert-Equal 2 $ambiguousControl.chatCenterCount "Ambiguous ControlView was replaced by RawView."
Assert-Equal 1 $rawSelectionProbe.calls "Ambiguous ControlView incorrectly fell back to RawView."
$emptyThenClosed = @(
  [ordered]@{ uiaView = "control"; chatCenterCount = 0; explorerSidebarCount = 0; hideSidebarButtonCount = 0; showSidebarButtonCount = 0; visitedElementCount = 3 },
  [ordered]@{ uiaView = "raw"; chatCenterCount = 1; explorerSidebarCount = 0; hideSidebarButtonCount = 0; showSidebarButtonCount = 1; visitedElementCount = 20 }
)
$closedRetryProbe = [pscustomobject]@{ index = 0 }
$closedRetry = Wait-FootprintChatTransitionDecision {
  $snapshot = $emptyThenClosed[$closedRetryProbe.index]
  $closedRetryProbe.index += 1
  return $snapshot
} 2 0
Assert-Equal "already-closed-noop" $closedRetry.decision.action "Empty UIA readiness did not retry to the exact closed state."
Assert-Equal 2 $closedRetry.attempts "Closed-state readiness retry count was not retained."
$ambiguousThenOpen = @(
  [ordered]@{ uiaView = "raw"; chatCenterCount = 1; explorerSidebarCount = 1; hideSidebarButtonCount = 2; showSidebarButtonCount = 0; visitedElementCount = 15 },
  [ordered]@{ uiaView = "raw"; chatCenterCount = 1; explorerSidebarCount = 1; hideSidebarButtonCount = 1; showSidebarButtonCount = 0; visitedElementCount = 25 }
)
$openRetryProbe = [pscustomobject]@{ index = 0 }
$openRetry = Wait-FootprintChatTransitionDecision {
  $snapshot = $ambiguousThenOpen[$openRetryProbe.index]
  $openRetryProbe.index += 1
  return $snapshot
} 2 0
Assert-Equal "invoke-hide-right-sidebar" $openRetry.decision.action "Ambiguous UIA readiness did not retry to the exact open state."
Assert-Equal 2 $openRetry.attempts "Open-state readiness retry count was not retained."
$invalidRetryProbe = [pscustomobject]@{ calls = 0 }
$invalidRetryError = $null
try {
  Wait-FootprintChatTransitionDecision {
    $invalidRetryProbe.calls += 1
    return [ordered]@{ uiaView = "control"; chatCenterCount = 1; explorerSidebarCount = 2; hideSidebarButtonCount = 0; showSidebarButtonCount = 0; visitedElementCount = 41 }
  } 3 0
} catch {
  $invalidRetryError = $_.Exception.Message
}
Assert-Equal 3 $invalidRetryProbe.calls "Permanent invalid readiness exceeded or skipped its bounded attempts."
Assert-True $invalidRetryError.Contains("final view=control, chat=1, explorer=2, hide=0, show=0, visited=41, traversalCap=4096") "Readiness timeout omitted final observed counts, chosen view, and traversal cap."
$throwingProviderProbe = [pscustomobject]@{ calls = 0 }
$throwingProviderError = $null
try {
  Wait-FootprintChatTransitionDecision {
    $throwingProviderProbe.calls += 1
    throw "root PID`r`ntitle mismatch"
  } 2 0
} catch {
  $throwingProviderError = $_.Exception.Message
}
Assert-Equal 2 $throwingProviderProbe.calls "Throwing snapshot provider exceeded or skipped its bounded attempts."
Assert-True $throwingProviderError.Contains("providerError=root PID title mismatch") "Readiness timeout swallowed or failed to sanitize the final snapshot-provider error."

$temporaryDirectory = Join-Path ([IO.Path]::GetTempPath()) ("sage-footprint-test-" + [Guid]::NewGuid().ToString("N"))
[void](New-Item -ItemType Directory -Path $temporaryDirectory)
try {
  $fakeExecutable = Join-Path $temporaryDirectory "sage.exe"
  [IO.File]::WriteAllBytes($fakeExecutable, [byte[]](1, 2, 3, 4))
  $actualHash = (Get-FileHash -LiteralPath $fakeExecutable -Algorithm SHA256).Hash.ToLowerInvariant()
  $bindingPath = Join-Path $temporaryDirectory "candidate.json"
  $manifestPath = Join-Path $temporaryDirectory "source-manifest.json"
  $manifest = [ordered]@{
    schemaVersion = 1
    aggregateConstruction = "SHA-256(path NUL sha256 NUL byteLength LF)"
    aggregateSha256 = ""
    fileCount = 1
    totalBytes = 4
    files = @([ordered]@{ path = "src/example.ts"; sha256 = ("d" * 64); bytes = 4 })
  }
  $manifest.aggregateSha256 = Get-FootprintSourceManifestAggregate $manifest
  $manifest | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $manifestPath -Encoding UTF8
  [ordered]@{ cargoProfile = "release"; cargoFeatures = @("custom-protocol", "diagnostics"); candidate = [ordered]@{
    executableSha256 = $actualHash
    aggregateSha256 = $manifest.aggregateSha256
    fileCount = $manifest.fileCount
    totalBytes = $manifest.totalBytes
    diagnosticsFeature = $true
  } } |
    ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $bindingPath -Encoding UTF8
  $binding = Test-FootprintCandidateBinding $fakeExecutable $actualHash $manifest.aggregateSha256 $bindingPath $manifestPath
  Assert-Equal $actualHash $binding.executableSha256 "Executable binding failed."
  Assert-Equal (Get-FileHash -LiteralPath $bindingPath -Algorithm SHA256).Hash.ToLowerInvariant() $binding.candidateBindingSha256 "Candidate binding bytes were not retained in evidence."
  Assert-Equal (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant() $binding.sourceManifestSha256 "Source-manifest bytes were not retained in evidence."
  Assert-True $binding.diagnosticsFeature "Diagnostics-feature provenance was not retained in the candidate binding."
  Assert-Equal "release" $binding.cargoProfile "Diagnostics candidate release profile was not retained."
  Assert-True ("custom-protocol" -in $binding.cargoFeatures) "Custom-protocol feature provenance was not retained."
  $missingCustomProtocolPath = Join-Path $temporaryDirectory "missing-custom-protocol.json"
  [ordered]@{ cargoProfile = "release"; cargoFeatures = @("diagnostics"); candidate = [ordered]@{
    executableSha256 = $actualHash
    aggregateSha256 = $manifest.aggregateSha256
    fileCount = $manifest.fileCount
    totalBytes = $manifest.totalBytes
    diagnosticsFeature = $true
  } } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $missingCustomProtocolPath -Encoding UTF8
  Assert-Throws {
    Test-FootprintCandidateBinding $fakeExecutable $actualHash $manifest.aggregateSha256 $missingCustomProtocolPath $manifestPath
  } "A diagnostics release binding without custom-protocol passed."
  $invalidProfiles = @(
    [pscustomobject]@{ name = "array"; value = [object[]]@("release") },
    [pscustomobject]@{ name = "object"; value = [ordered]@{ value = "release" } },
    [pscustomobject]@{ name = "number"; value = 1 }
  )
  foreach ($invalidProfile in $invalidProfiles) {
    $invalidProfilePath = Join-Path $temporaryDirectory ("invalid-profile-{0}.json" -f $invalidProfile.name)
    [ordered]@{ cargoProfile = $invalidProfile.value; cargoFeatures = @("custom-protocol", "diagnostics"); candidate = [ordered]@{
      executableSha256 = $actualHash
      aggregateSha256 = $manifest.aggregateSha256
      fileCount = $manifest.fileCount
      totalBytes = $manifest.totalBytes
      diagnosticsFeature = $true
    } } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $invalidProfilePath -Encoding UTF8
    Assert-Throws {
      Test-FootprintCandidateBinding $fakeExecutable $actualHash $manifest.aggregateSha256 $invalidProfilePath $manifestPath
    } "A non-string cargoProfile '$($invalidProfile.name)' passed."
  }
  Assert-Throws {
    Test-FootprintCandidateBinding $fakeExecutable ("c" * 64) $manifest.aggregateSha256 $bindingPath $manifestPath
  } "A relabeled candidate executable passed."
  $relabeledBindingPath = Join-Path $temporaryDirectory "relabeled-candidate.json"
  [ordered]@{ cargoProfile = "release"; cargoFeatures = @("custom-protocol", "diagnostics"); candidate = [ordered]@{
    executableSha256 = $actualHash
    aggregateSha256 = ("c" * 64)
    fileCount = 1
    totalBytes = 4
    diagnosticsFeature = $true
  } } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $relabeledBindingPath -Encoding UTF8
  Assert-Throws {
    Test-FootprintCandidateBinding $fakeExecutable $actualHash ("c" * 64) $relabeledBindingPath $manifestPath
  } "A source aggregate relabeled in both arguments and binding passed without manifest proof."
  $corruptManifestPath = Join-Path $temporaryDirectory "corrupt-source-manifest.json"
  $corruptManifest = [ordered]@{} + $manifest
  $corruptManifest.aggregateSha256 = "c" * 64
  $corruptManifest | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $corruptManifestPath -Encoding UTF8
  Assert-Throws {
    Test-FootprintCandidateBinding $fakeExecutable $actualHash $manifest.aggregateSha256 $bindingPath $corruptManifestPath
  } "A retained manifest with an internal aggregate mismatch passed."
  $nonDiagnosticBindingPath = Join-Path $temporaryDirectory "non-diagnostic-candidate.json"
  [ordered]@{ candidate = [ordered]@{
    executableSha256 = $actualHash
    aggregateSha256 = $manifest.aggregateSha256
    fileCount = $manifest.fileCount
    totalBytes = $manifest.totalBytes
    diagnosticsFeature = $false
  } } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $nonDiagnosticBindingPath -Encoding UTF8
  $nonDiagnosticBinding = Test-FootprintCandidateBinding $fakeExecutable $actualHash $manifest.aggregateSha256 $nonDiagnosticBindingPath $manifestPath
  Assert-True (-not $nonDiagnosticBinding.diagnosticsFeature) "Production-candidate capability provenance was not retained."
  Assert-Throws {
    Assert-FootprintDiagnosticCampaignInputs $nonDiagnosticBinding ""
  } "A production candidate entered the diagnostics-only campaign."
  Assert-Throws {
    Assert-FootprintDiagnosticCampaignInputs $nonDiagnosticBinding "C:\isolated-diagnostics"
  } "A production candidate accepted the diagnostics-only data override."
  Assert-Throws {
    Assert-FootprintDiagnosticCampaignInputs $binding ""
  } "A diagnostics candidate passed without an explicit isolated data directory."
  Assert-True (Assert-FootprintDiagnosticCampaignInputs $binding "C:\isolated-diagnostics") "A diagnostics candidate rejected an explicit isolated data directory."
  $stringCapabilityBindingPath = Join-Path $temporaryDirectory "string-capability-candidate.json"
  [ordered]@{ cargoProfile = "release"; cargoFeatures = @("custom-protocol", "diagnostics"); candidate = [ordered]@{
    executableSha256 = $actualHash
    aggregateSha256 = $manifest.aggregateSha256
    fileCount = $manifest.fileCount
    totalBytes = $manifest.totalBytes
    diagnosticsFeature = "false"
  } } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $stringCapabilityBindingPath -Encoding UTF8
  Assert-Throws {
    Test-FootprintCandidateBinding $fakeExecutable $actualHash $manifest.aggregateSha256 $stringCapabilityBindingPath $manifestPath
  } "A non-boolean diagnostics capability entered candidate binding."
  $aliasManifest = [ordered]@{} + $manifest
  $aliasManifest.files = @([ordered]@{ path = "src//example.ts"; sha256 = ("d" * 64); bytes = 4 })
  Assert-Throws {
    Get-FootprintSourceManifestAggregate $aliasManifest
  } "A noncanonical source-manifest path alias passed."
  $overByteManifest = [ordered]@{} + $manifest
  $overByteManifest.totalBytes = 512MB + 1
  $overByteManifest.files = @([ordered]@{ path = "src/example.ts"; sha256 = ("d" * 64); bytes = 512MB + 1 })
  Assert-Throws {
    Get-FootprintSourceManifestAggregate $overByteManifest
  } "An over-limit source-manifest byte aggregate passed."
  $overCountManifest = [ordered]@{} + $manifest
  $overCountManifest.fileCount = 4097
  $overCountManifest.files = @(0..4096 | ForEach-Object {
    [ordered]@{ path = "src/$($_.ToString('D4')).ts"; sha256 = ("d" * 64); bytes = 1 }
  })
  Assert-Throws {
    Get-FootprintSourceManifestAggregate $overCountManifest
  } "An over-limit source-manifest file count passed."
  [void](Assert-FootprintExecutableHash $fakeExecutable $actualHash)
  [IO.File]::WriteAllBytes($fakeExecutable, [byte[]](4, 3, 2, 1))
  Assert-Throws {
    Assert-FootprintExecutableHash $fakeExecutable $actualHash
  } "Per-launch executable replacement was not rejected."
} finally {
  Remove-Item -LiteralPath $temporaryDirectory -Recurse -Force
}

$workspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$provenanceRoot = Join-Path $workspaceRoot ("local-evidence/footprint-contract-" + [Guid]::NewGuid().ToString("N"))
$diagnosticData = Join-Path $provenanceRoot "diagnostic-data"
$provenanceOutput = Join-Path $provenanceRoot "output"
$fixtureRoot = Join-Path $provenanceRoot "fixture-project"
[void](New-Item -ItemType Directory -Force -Path $diagnosticData)
[void](New-Item -ItemType Directory -Force -Path $provenanceOutput)
[void](New-Item -ItemType Directory -Force -Path $fixtureRoot)
try {
  "fixture" | Set-Content -LiteralPath (Join-Path $fixtureRoot "README.txt") -Encoding UTF8
  $freshProvenance = Get-FootprintDiagnosticDataProvenance $diagnosticData $workspaceRoot $provenanceOutput
  $fixtureProvenance = Get-FootprintFixtureProjectProvenance $fixtureRoot $workspaceRoot $diagnosticData $provenanceOutput
  Assert-Equal "native-project-seed" $fixtureProvenance.purpose "Fixture project provenance purpose was not explicit."
  Assert-Equal 1 $fixtureProvenance.fileCount "Fixture project seed file count was not retained."
  Assert-Equal 64 $fixtureProvenance.pathSha256.Length "Fixture project canonical path identity was not hashed."
  Assert-True (Assert-FootprintFreshDiagnosticData $freshProvenance) "A fresh scenario data directory was rejected."
  Assert-Throws { Assert-FootprintDiagnosticStateChanged $freshProvenance $freshProvenance } "A mislabeled production launch passed without creating isolated database state."
  [IO.File]::WriteAllBytes((Join-Path $diagnosticData "sage.db"), [byte[]](7, 8, 9))
  $provenance = Get-FootprintDiagnosticDataProvenance $diagnosticData $workspaceRoot $provenanceOutput
  Assert-True (Assert-FootprintDiagnosticStateChanged $freshProvenance $provenance) "Created isolated database state was not accepted."
  Assert-Throws { Assert-FootprintFreshDiagnosticData $provenance } "A reused diagnostics directory passed as fresh."
  Assert-True $provenance.withinIgnoredLocalEvidence "Diagnostics data was not proven inside ignored local evidence."
  Assert-Equal "isolated-diagnostics" $provenance.purpose "Diagnostics provenance purpose was not explicit."
  Assert-Equal 64 $provenance.pathSha256.Length "Diagnostics path identity was not hashed."
  Assert-Equal 3 $provenance.stateFiles.Count "Diagnostics database triple provenance is incomplete."
  Assert-Equal 3 $provenance.stateFiles[0].bytes "Diagnostics database bytes were not retained."
  Assert-Throws {
    Get-FootprintDiagnosticDataProvenance $diagnosticData $workspaceRoot $provenanceRoot
  } "Diagnostics data was allowed inside the campaign output boundary."
  Assert-Throws {
    Get-FootprintDiagnosticDataProvenance $provenanceOutput $workspaceRoot $provenanceOutput
  } "The campaign output directory was accepted as diagnostics data."
  $previousDiagnosticEnvironment = [Environment]::GetEnvironmentVariable("SAGE_DIAGNOSTIC_DATA_DIR", "Process")
  try {
    [Environment]::SetEnvironmentVariable("SAGE_DIAGNOSTIC_DATA_DIR", $diagnosticData, "Process")
    Assert-Throws { Assert-FootprintDiagnosticEnvironmentClear } "An inherited diagnostics data override did not fail closed."
  } finally {
    [Environment]::SetEnvironmentVariable("SAGE_DIAGNOSTIC_DATA_DIR", $previousDiagnosticEnvironment, "Process")
  }
  [void](Assert-FootprintDiagnosticEnvironmentClear)
  $previousFixtureEnvironment = [Environment]::GetEnvironmentVariable("SAGE_PROJECT_ROOT", "Process")
  try {
    [Environment]::SetEnvironmentVariable("SAGE_PROJECT_ROOT", $fixtureRoot, "Process")
    Assert-Throws { Assert-FootprintFixtureEnvironmentClear } "An inherited fixture project override did not fail closed."
  } finally {
    [Environment]::SetEnvironmentVariable("SAGE_PROJECT_ROOT", $previousFixtureEnvironment, "Process")
  }
  [void](Assert-FootprintFixtureEnvironmentClear)
  Assert-Throws {
    Get-FootprintFixtureProjectProvenance $diagnosticData $workspaceRoot $diagnosticData $provenanceOutput
  } "A fixture project overlapping diagnostics data passed."
  $outsideFixture = Join-Path ([IO.Path]::GetTempPath()) ("sage-outside-fixture-" + [Guid]::NewGuid().ToString("N"))
  [void](New-Item -ItemType Directory -Path $outsideFixture)
  try {
    "outside" | Set-Content -LiteralPath (Join-Path $outsideFixture "seed.txt") -Encoding UTF8
    Assert-Throws {
      Get-FootprintFixtureProjectProvenance $outsideFixture $workspaceRoot $diagnosticData $provenanceOutput
    } "A fixture project outside ignored local evidence passed."
  } finally {
    Remove-Item -LiteralPath $outsideFixture -Recurse -Force
  }
  function New-TestNormalInventory([string]$Digest = ("a" * 64), [int]$SchemaVersion = 2, [string]$PathDigest = ("c" * 64)) {
    return [ordered]@{
      schemaVersion = $SchemaVersion
      durableRoaming = [ordered]@{
        label = "roaming-durable-ai.sage.desktop"
        pathSha256 = $PathDigest
        exists = $true
        entryCount = 1
        totalFileBytes = 4
        aggregateSha256 = $Digest
      }
      durableAggregateSha256 = $Digest
      disposableLocalCache = [ordered]@{
        label = "local-webview-cache-ai.sage.desktop"
        pathSha256 = "d" * 64
        existsAtCapture = $false
        policy = "disposable-uncontrolled-not-an-isolation-gate"
        expectedChurn = "EBWebView and Tauri runtime cache may change during an isolated SQLite campaign"
      }
    }
  }
  $normalBefore = New-TestNormalInventory
  $normalSame = New-TestNormalInventory
  $normalChanged = New-TestNormalInventory ("b" * 64)
  Assert-True (Assert-FootprintNormalDataUnchanged $normalBefore $normalSame) "Identical durable Roaming inventories were rejected."
  Assert-Throws { Assert-FootprintNormalDataUnchanged $normalBefore $normalChanged } "Durable Roaming mutation did not fail isolation."
  Assert-Throws { Assert-FootprintNormalDataUnchanged $normalBefore (New-TestNormalInventory ("a" * 64) 1) } "A stale normal-data inventory schema was accepted."
  Assert-Throws { Assert-FootprintNormalDataUnchanged $normalBefore (New-TestNormalInventory ("a" * 64) 2 ("e" * 64)) } "A different normal-data path identity was accepted."
  $guardedSuccess = Invoke-FootprintNormalDataGuardedAction $normalBefore { return "ok" } { return $normalSame }
  Assert-Equal "ok" $guardedSuccess.result "A successful guarded action lost its result."
  Assert-Throws { Invoke-FootprintNormalDataGuardedAction $normalBefore { return "ok" } { throw "capture failed" } } "A failed post-action inventory capture was accepted."
  $setupGuardProbe = [pscustomobject]@{ ran = $false }
  $guardedFailure = $null
  try {
    Invoke-FootprintNormalDataGuardedAction $normalBefore `
      { throw "simulated setup failure after mutation" } `
      { $setupGuardProbe.ran = $true; return $normalChanged }
  } catch {
    $guardedFailure = $_.Exception.Message
  }
  Assert-True $setupGuardProbe.ran "Setup failure bypassed the normal-data post-inventory guard."
  Assert-True ($guardedFailure.Contains("simulated setup failure after mutation")) "Guarded setup lost the original failure context."
  Assert-True ($guardedFailure.Contains("Normal-data isolation guard also failed")) "Guarded setup did not fail on the simulated normal-data mutation."
} finally {
  Remove-Item -LiteralPath $provenanceRoot -Recurse -Force
}

$inventoryTestRoot = Join-Path ([IO.Path]::GetTempPath()) ("sage-inventory-test-" + [Guid]::NewGuid().ToString("N"))
$absentInventory = Get-FootprintBoundedDirectoryInventory $inventoryTestRoot "inventory-test"
Assert-True (-not $absentInventory.exists) "An absent directory inventory was reported as present."
[void][IO.Directory]::CreateDirectory($inventoryTestRoot)
try {
  [IO.File]::WriteAllText((Join-Path $inventoryTestRoot "value.txt"), "before")
  $presentInventory = Get-FootprintBoundedDirectoryInventory $inventoryTestRoot "inventory-test"
  Assert-True $presentInventory.exists "A present directory inventory was reported as absent."
  Assert-Equal 1 $presentInventory.entryCount "A bounded directory inventory counted the wrong entries."
  [IO.File]::WriteAllText((Join-Path $inventoryTestRoot "value.txt"), "after")
  $changedInventory = Get-FootprintBoundedDirectoryInventory $inventoryTestRoot "inventory-test"
  Assert-True ($changedInventory.aggregateSha256 -ne $presentInventory.aggregateSha256) "A file-content mutation retained the same inventory digest."
} finally {
  Remove-Item -LiteralPath $inventoryTestRoot -Recurse -Force
}

$invalidObservationRequest = Join-Path ([IO.Path]::GetTempPath()) ("sage-c0b-invalid-" + [Guid]::NewGuid().ToString("N") + ".json")
try {
  [IO.File]::WriteAllText($invalidObservationRequest, '{"schemaVersion":1}', [Text.UTF8Encoding]::new($false))
  Assert-Throws { Invoke-C0BObservationRequest $invalidObservationRequest } "The C0-B collector accepted an incomplete launch request."
} finally {
  Remove-Item -LiteralPath $invalidObservationRequest -Force -ErrorAction SilentlyContinue
}

$nodeTestRoot = Join-Path ([IO.Path]::GetTempPath()) ("sage-bounded-node-" + [Guid]::NewGuid().ToString("N"))
[void](New-Item -ItemType Directory -Path $nodeTestRoot)
try {
  $nodeExecutable = (Get-Command node.exe -CommandType Application -ErrorAction Stop).Source
  $nodeRunner = (Resolve-Path (Join-Path $PSScriptRoot "footprint/bounded-node-runner.mjs")).Path
  $successScript = Join-Path $nodeTestRoot "success.mjs"
  $failureScript = Join-Path $nodeTestRoot "failure.mjs"
  $timeoutScript = Join-Path $nodeTestRoot "timeout.mjs"
  'process.stdout.write("bounded-ok");' | Set-Content -LiteralPath $successScript -Encoding UTF8
  'process.stderr.write("bounded-failure"); process.exit(7);' | Set-Content -LiteralPath $failureScript -Encoding UTF8
  'setTimeout(() => process.exit(0), 2000);' | Set-Content -LiteralPath $timeoutScript -Encoding UTF8
  $boundedSuccess = Invoke-FootprintBoundedProcess $nodeExecutable $nodeRunner $nodeExecutable @($successScript) (Join-Path $nodeTestRoot "success.json") 5000 1024
  Assert-Equal 0 $boundedSuccess.exitCode "Bounded Node success lost the exact exit code."
  Assert-Equal "bounded-ok" $boundedSuccess.stdout "Bounded Node success did not capture stdout."
  Assert-Throws {
    Invoke-FootprintBoundedProcess $nodeExecutable $nodeRunner $nodeExecutable @($failureScript) (Join-Path $nodeTestRoot "failure.json") 5000 1024
  } "A nonzero bounded Node process passed."
  Assert-Throws {
    Invoke-FootprintBoundedProcess $nodeExecutable $nodeRunner $nodeExecutable @($timeoutScript) (Join-Path $nodeTestRoot "timeout.json") 100 1024
  } "A timed-out bounded Node process passed."
  $powerShellExecutable = (Get-Command powershell.exe -CommandType Application -ErrorAction Stop).Source
  $matrixChild = Invoke-FootprintBoundedProcess $nodeExecutable $nodeRunner $powerShellExecutable `
    @("-NoProfile", "-Command", "Write-Output matrix-child-ok; exit 0") `
    (Join-Path $nodeTestRoot "matrix-success.json") 5000 1024
  Assert-Equal 0 $matrixChild.exitCode "Bounded matrix child lost the exact PowerShell exit code."
  Assert-True $matrixChild.stdout.Contains("matrix-child-ok") "Bounded matrix child did not capture PowerShell stdout."
  Assert-Throws {
    Invoke-FootprintBoundedProcess $nodeExecutable $nodeRunner $powerShellExecutable `
      @("-NoProfile", "-Command", "exit 9") (Join-Path $nodeTestRoot "matrix-failure.json") 5000 1024
  } "A nonzero bounded matrix PowerShell child passed."
  Assert-Throws {
    Invoke-FootprintBoundedProcess $nodeExecutable $nodeRunner $powerShellExecutable `
      @("-NoProfile", "-Command", "Start-Sleep -Seconds 2") (Join-Path $nodeTestRoot "matrix-timeout.json") 100 1024
  } "A timed-out bounded matrix PowerShell child passed."
} finally {
  Remove-Item -LiteralPath $nodeTestRoot -Recurse -Force
}

$harnessFiles = @(
  Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot "footprint") -File |
    Where-Object Extension -in @(".psm1", ".mjs")
) + @(
  (Get-Item -LiteralPath (Join-Path $PSScriptRoot "measure-windows-footprint.ps1"))
  (Get-Item -LiteralPath (Join-Path $PSScriptRoot "measure-windows-footprint-matrix.ps1"))
  (Get-Item -LiteralPath (Join-Path $PSScriptRoot "summarize-windows-footprint-matrix.ps1"))
)
$forbidden = @(
  ("Set" + "CursorPos"), ("mouse" + "_event"), ("Send" + "Input"),
  ("keybd" + "_event"), ("Send" + "Keys"), ("user" + "32"),
  ("Copy" + "FromScreen"), ("Screen" + "Point"), ("Css" + "X"), ("Css" + "Y")
)
foreach ($file in $harnessFiles) {
  $content = Get-Content -LiteralPath $file.FullName -Raw
  foreach ($token in $forbidden) {
    Assert-True ($content.IndexOf($token, [StringComparison]::OrdinalIgnoreCase) -lt 0) "Forbidden desktop input/coordinate token '$token' remains in $($file.Name)."
  }
}

$measureSource = Get-Content -LiteralPath (Join-Path $PSScriptRoot "measure-windows-footprint.ps1") -Raw
$matrixSource = Get-Content -LiteralPath (Join-Path $PSScriptRoot "measure-windows-footprint-matrix.ps1") -Raw
$setupSource = Get-Content -LiteralPath (Join-Path $PSScriptRoot "footprint/prepare-sage-scenario.mjs") -Raw
$windowsFootprintSource = Get-Content -LiteralPath (Join-Path $PSScriptRoot "footprint/WindowsFootprint.psm1") -Raw
$processIdentitySource = Get-Content -LiteralPath (Join-Path $PSScriptRoot "footprint/ProcessIdentity.psm1") -Raw
$scenarioEvidenceSource = Get-Content -LiteralPath (Join-Path $PSScriptRoot "footprint/ScenarioEvidence.psm1") -Raw
$disposableProfileSource = Get-Content -LiteralPath (Join-Path $PSScriptRoot "footprint/DisposableProfile.psm1") -Raw
Assert-True $windowsFootprintSource.Contains("Footprint sample captured no core candidate process") "Zero-core sampling is not rejected with a typed diagnostic."
Assert-True $processIdentitySource.Contains('Get-CimInstance Win32_Process -Property @(') "Process sampling is not limited to the fields used by the cadence collector."
Assert-True $windowsFootprintSource.Contains('privateBytes = [long]$member.PrivatePageCount') "Cadence sampling reopens each process instead of using one bounded CIM observation."
Assert-True $windowsFootprintSource.Contains('$records += [pscustomobject][ordered]@{') "Process samples are not represented as measurable property objects."
Assert-True $windowsFootprintSource.Contains('$excludedPrivateBytes = [long]0') "Empty excluded-role samples do not have a deterministic zero-byte aggregate."
Assert-True (-not $windowsFootprintSource.Contains('Measure-Object privateBytes -Sum')) "Sample aggregation still depends on missing strict-mode Sum properties."
Assert-True $scenarioEvidenceSource.Contains('processId = $ExpectedProcessId') "Live UIA evidence does not use the acceptance process identity field."
Assert-True $scenarioEvidenceSource.Contains("Measured Chat state retained an Explorer heavy view") "Live Chat evidence does not reject a mounted Explorer heavy view."
Assert-True $scenarioEvidenceSource.Contains("[Windows.Automation.InvokePattern]::Pattern") "Measured Chat launch does not use semantic UI Automation invocation."
Assert-True $scenarioEvidenceSource.Contains('[int]$current.Current.ProcessId -eq $ExpectedProcessId') "Measured Chat transition is not bound to the exact process identity."
Assert-True $disposableProfileSource.Contains("Normal durable Roaming Sage state changed during an isolated launch") "Normal durable-data mutation does not fail closed."
Assert-True $disposableProfileSource.Contains("disposable-uncontrolled-not-an-isolation-gate") "Local WebView cache is not explicitly recorded as disposable and uncontrolled."
Assert-True $setupSource.Contains('button[aria-label="Show right sidebar"]') "Chat setup does not prove that the right sidebar is closed."
Assert-True $setupSource.Contains("finalState.explorerSidebarCount === 0") "Chat setup does not prove that the Explorer heavy view is unmounted."
Assert-True $setupSource.Contains("if(chats.length!==1)return false") "Setup does not require exactly one native-seeded project Chat."
Assert-True $setupSource.Contains("finalState.projectChatCount === 1") "Final setup evidence does not retain the exact project Chat count."
Assert-True $measureSource.Contains('Get-FootprintCpuMeasurement $first $last') "CPU measurement is not derived from timestamped samples."
Assert-True (-not $measureSource.Contains('$SampleIntervalMs * ($SampleCount - 1) / 1000')) "CPU measurement still derives wall duration from the nominal sleep interval."
Assert-True $measureSource.Contains('Assert-FootprintDiagnosticCampaignInputs $binding $DiagnosticDataDirectory') "The diagnostics campaign does not fail closed on candidate capability/data provenance."
Assert-True $measureSource.Contains('Assert-FootprintDiagnosticStateChanged $diagnosticDataInitial $diagnosticDataAfterSetup') "The diagnostics campaign trusts metadata without behavioral state proof."
Assert-True $measureSource.Contains('Invoke-FootprintBoundedProcess $resolvedNode $boundedNodeRunner $resolvedNode') "Semantic setup does not use bounded exact-path Node execution."
Assert-True (-not $measureSource.Contains('& $resolvedNode $setupHelper')) "Semantic setup still uses ambient native-command exit state."
Assert-True (-not $measureSource.Contains('$LASTEXITCODE')) "Semantic setup still trusts shared LASTEXITCODE state."
Assert-True $matrixSource.Contains('Invoke-FootprintBoundedProcess $resolvedNode $runner $resolvedPowerShell $arguments') "Matrix scenarios do not use bounded exact-path PowerShell execution."
Assert-True (-not $matrixSource.Contains('& powershell.exe')) "Matrix still launches PowerShell through ambient native invocation."
Assert-True (-not $matrixSource.Contains('$LASTEXITCODE')) "Matrix still trusts shared LASTEXITCODE state."
Assert-True $matrixSource.Contains('120000 1MB') "Matrix child timeout/output bounds are not explicit."
Assert-True $setupSource.Contains('const deadline = Date.now() + 20_000') "Target readiness is not bounded to 20 seconds."
Assert-True $setupSource.Contains('target discovery response exceeded 64 KiB') "Target discovery response bytes are not bounded before parsing."
Assert-True $setupSource.Contains('target discovery exceeded 32 entries') "Target discovery count is not bounded."
Assert-True $setupSource.Contains('targetSummaries: targetDiagnostics') "Setup failure evidence omits sanitized target summaries."
Assert-True $setupSource.Contains('target.url === "http://tauri.localhost/"') "Target discovery does not retain the exact authoritative Tauri origin."
Assert-True $setupSource.Contains('pages.length === 1 && pages[0].title === "Sage"') "Target readiness does not distinguish page selection from title readiness."
Assert-True $measureSource.Contains('Invoke-FootprintNormalDataGuardedAction $normalDataInitial') "Setup failure paths do not always run the normal-data isolation guard."
Assert-True $measureSource.Contains('disposableProfileModuleSha256 =') "Legacy footprint evidence does not bind the normal-data guard module."
Assert-True $measureSource.Contains('[Environment]::SetEnvironmentVariable("SAGE_PROJECT_ROOT", $fixtureProject.path, "Process")') "Native fixture seeding is not process-scoped to the exact setup launch."
Assert-True $measureSource.Contains('[Environment]::SetEnvironmentVariable("SAGE_PROJECT_ROOT", $null, "Process")') "Native fixture seeding is not always cleared."
Assert-True ([regex]::Matches($measureSource, 'Start-ExactFootprintProcess -SeedProject').Count -eq 1) "Fixture seeding is not limited to the initial setup launch."
Assert-True $measureSource.Contains('Invoke-FootprintMeasuredScenarioTransition $window.handle $process.Id $Scenario') "Measured launches do not perform the exact-process scenario transition."
Assert-True $measureSource.Contains('diagnosticDataDirectorySha256 = $diagnosticDataInitial.pathSha256') "Measured evidence does not retain isolated data-directory identity."
Assert-True $matrixSource.Contains('$arguments += @("-DiagnosticDataDirectory", $scenarioDataDirectory)') "The matrix harness does not forward scenario-specific isolated data-directory provenance."
Assert-True $matrixSource.Contains('$scenarioDataDirectory = Join-Path $DiagnosticDataDirectory $scenario') "Chat and Explorer do not receive disjoint scenario data directories."
Assert-True $matrixSource.Contains('"-FixtureProjectRoot"') "The matrix harness does not forward the bounded fixture project root."
Assert-True $windowsFootprintSource.Contains("Core process identity membership changed during CPU sampling") "CPU process churn does not invalidate measurement."
Assert-True ($measureSource.IndexOf("Invoke-InstrumentedScenarioSetup", [StringComparison]::Ordinal) -lt $measureSource.IndexOf("while (`$validCount", [StringComparison]::Ordinal)) "Setup is not ordered before measured launches."
Assert-True (($measureSource.Split("Wait-FootprintScenarioUiaEvidence").Count - 1) -ge 2) "Measured launches do not verify semantic state before and after sampling."
Assert-True (($measureSource.Split("Assert-FootprintExecutableHash").Count - 1) -ge 1) "Per-launch executable rehash is not wired into process launch."
Assert-True ([regex]::Matches($measureSource, [regex]::Escape('Stop-FootprintLaunchedProcessObject $process')).Count -eq 2) "Setup and measured launches do not both wire exact process-object fallback cleanup."
Assert-True ([regex]::Matches($measureSource, [regex]::Escape('Stop-FootprintLaunchedProcessObject $launched')).Count -eq 1) "Post-launch candidate validation failure lacks exact process-object cleanup."
Assert-True ([regex]::Matches($measureSource, [regex]::Escape('Stop-FootprintTree $identity $known 10')).Count -eq 2) "Setup and measured launches do not both use the bounded durable-close grace."

$parseFiles = @($harnessFiles | Where-Object Extension -in @(".ps1", ".psm1"))
foreach ($file in $parseFiles) {
  $tokens = $null
  $errors = $null
  [void][Management.Automation.Language.Parser]::ParseFile($file.FullName, [ref]$tokens, [ref]$errors)
  Assert-Equal 0 $errors.Count "PowerShell parser errors in $($file.Name)."
}

Write-Host "footprint harness tests: passed ($script:CheckCount checks)"
