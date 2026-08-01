$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

Import-Module (Join-Path $PSScriptRoot "footprint/WindowsFootprint.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "footprint/ProcessIdentity.psm1") -Force
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

Assert-Equal 2.5 (Get-FootprintMedian @(4.0, 1.0)) "Even median failed."
Assert-Equal 3.0 (Get-FootprintMedian @(3.0, 1.0, 9.0)) "Odd median failed."
Assert-Equal 5.0 (Get-FootprintPercentile @(1.0, 2.0, 3.0, 4.0, 5.0) 0.95) "Nearest-rank P95 failed."
Assert-Equal 0.5 (Get-FootprintCpuPercent 10.0 10.02 4.0) "Raw CPU arithmetic failed."

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

$rows = @(
  [pscustomobject]@{ ProcessId = 10; ParentProcessId = 1 },
  [pscustomobject]@{ ProcessId = 11; ParentProcessId = 10 },
  [pscustomobject]@{ ProcessId = 12; ParentProcessId = 11 },
  [pscustomobject]@{ ProcessId = 20; ParentProcessId = 1 }
)
$descendants = @(Get-FootprintDescendants 10 $rows)
Assert-Equal 3 $descendants.Count "Descendant count failed."
Assert-Equal "10,11,12" (($descendants | ForEach-Object ProcessId) -join ",") "Descendant membership failed."
Assert-Throws { Get-FootprintDescendants 10 $rows 2 128 } "An over-limit system process snapshot was collected."
Assert-Throws { Get-FootprintDescendants 10 $rows 4096 2 } "An over-limit candidate descendant set was retained."

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
Assert-True $rootReusedChildAlive.identityLost "Root PID reuse did not fail closed."
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
$setup = [ordered]@{
  scenario = "chat"; executed = $true; semanticVerified = $true; processId = 10
  title = "Sage"; origin = "http://tauri.localhost/"; executablePath = $expectedPath
  executableSha256 = $expectedExecutableHash; sourceAggregateSha256 = $expectedSourceHash
}
$chatMeasured = [ordered]@{
  semanticVerified = $true; processId = 20; rootTitle = "Sage"
  chatCenterCount = 1; explorerSidebarCount = 0
}
$explorerMeasured = [ordered]@{
  semanticVerified = $true; processId = 21; rootTitle = "Sage"
  chatCenterCount = 1; explorerSidebarCount = 1
}
$cleanSamples = @([ordered]@{ unknownProcessCount = 0; processes = @() })
Assert-True (Assert-FootprintScenarioAcceptance "chat" $setup $chatMeasured $cleanSamples $expectedPath $expectedExecutableHash $expectedSourceHash) "Chat scenario contract failed."
$explorerSetup = [ordered]@{
  scenario = "explorer"; executed = $true; semanticVerified = $true; processId = 10
  title = "Sage"; origin = "http://tauri.localhost/"; executablePath = $expectedPath
  executableSha256 = $expectedExecutableHash; sourceAggregateSha256 = $expectedSourceHash
}
Assert-True (Assert-FootprintScenarioAcceptance "explorer" $explorerSetup $explorerMeasured $cleanSamples $expectedPath $expectedExecutableHash $expectedSourceHash) "Explorer scenario contract failed."
Assert-Throws {
  Assert-FootprintScenarioAcceptance "chat" ([ordered]@{ scenario = "chat" }) $chatMeasured $cleanSamples $expectedPath $expectedExecutableHash $expectedSourceHash
} "A scenario label without setup evidence passed."
$sameProcess = [ordered]@{
  semanticVerified = $true; processId = 10; rootTitle = "Sage"
  chatCenterCount = 1; explorerSidebarCount = 0
}
Assert-Throws {
  Assert-FootprintScenarioAcceptance "chat" $setup $sameProcess $cleanSamples $expectedPath $expectedExecutableHash $expectedSourceHash
} "Setup and measured launches were not required to be distinct."
$ambiguousExplorer = [ordered]@{
  semanticVerified = $true; processId = 21; rootTitle = "Sage"
  chatCenterCount = 1; explorerSidebarCount = 2
}
Assert-Throws {
  Assert-FootprintScenarioAcceptance "explorer" $explorerSetup $ambiguousExplorer $cleanSamples $expectedPath $expectedExecutableHash $expectedSourceHash
} "An ambiguous Explorer accessibility tree passed."
$unknownSamples = @([ordered]@{ unknownProcessCount = 1; processes = @() })
Assert-Throws {
  Assert-FootprintScenarioAcceptance "chat" $setup $chatMeasured $unknownSamples $expectedPath $expectedExecutableHash $expectedSourceHash
} "An unknown descendant did not invalidate acceptance."
$debugSamples = @([ordered]@{
  unknownProcessCount = 0
  processes = @([ordered]@{ commandLine = "msedgewebview2.exe --remote-debugging-port=9222" })
})
Assert-Throws {
  Assert-FootprintScenarioAcceptance "chat" $setup $chatMeasured $debugSamples $expectedPath $expectedExecutableHash $expectedSourceHash
} "A measured debug descendant did not invalidate acceptance."

foreach ($scenario in @("chat", "explorer")) {
  $contract = Get-FootprintScenarioContract $scenario
  Assert-True ($contract.setupRequired -and $contract.setupVerificationRequired -and $contract.measuredVerificationRequired) "$scenario omitted a required phase."
}

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
  [ordered]@{ candidate = [ordered]@{
    executableSha256 = $actualHash
    aggregateSha256 = $manifest.aggregateSha256
    fileCount = $manifest.fileCount
    totalBytes = $manifest.totalBytes
  } } |
    ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $bindingPath -Encoding UTF8
  $binding = Test-FootprintCandidateBinding $fakeExecutable $actualHash $manifest.aggregateSha256 $bindingPath $manifestPath
  Assert-Equal $actualHash $binding.executableSha256 "Executable binding failed."
  Assert-Equal (Get-FileHash -LiteralPath $bindingPath -Algorithm SHA256).Hash.ToLowerInvariant() $binding.candidateBindingSha256 "Candidate binding bytes were not retained in evidence."
  Assert-Equal (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant() $binding.sourceManifestSha256 "Source-manifest bytes were not retained in evidence."
  Assert-Throws {
    Test-FootprintCandidateBinding $fakeExecutable ("c" * 64) $manifest.aggregateSha256 $bindingPath $manifestPath
  } "A relabeled candidate executable passed."
  $relabeledBindingPath = Join-Path $temporaryDirectory "relabeled-candidate.json"
  [ordered]@{ candidate = [ordered]@{
    executableSha256 = $actualHash
    aggregateSha256 = ("c" * 64)
    fileCount = 1
    totalBytes = 4
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
Assert-True ($measureSource.IndexOf("Invoke-InstrumentedScenarioSetup", [StringComparison]::Ordinal) -lt $measureSource.IndexOf("while (`$validCount", [StringComparison]::Ordinal)) "Setup is not ordered before measured launches."
Assert-True (($measureSource.Split("Wait-FootprintScenarioUiaEvidence").Count - 1) -ge 2) "Measured launches do not verify semantic state before and after sampling."
Assert-True (($measureSource.Split("Assert-FootprintExecutableHash").Count - 1) -ge 1) "Per-launch executable rehash is not wired into process launch."
Assert-True ([regex]::Matches($measureSource, [regex]::Escape('Stop-FootprintLaunchedProcessObject $process')).Count -eq 2) "Setup and measured launches do not both wire exact process-object fallback cleanup."
Assert-True ([regex]::Matches($measureSource, [regex]::Escape('Stop-FootprintLaunchedProcessObject $launched')).Count -eq 1) "Post-launch candidate validation failure lacks exact process-object cleanup."

$parseFiles = @($harnessFiles | Where-Object Extension -in @(".ps1", ".psm1"))
foreach ($file in $parseFiles) {
  $tokens = $null
  $errors = $null
  [void][Management.Automation.Language.Parser]::ParseFile($file.FullName, [ref]$tokens, [ref]$errors)
  Assert-Equal 0 $errors.Count "PowerShell parser errors in $($file.Name)."
}

Write-Host "footprint harness tests: passed ($script:CheckCount checks)"
