$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

Import-Module (Join-Path $PSScriptRoot "footprint/WindowsFootprint.psm1") -Force
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
  [ordered]@{ candidate = [ordered]@{ executableSha256 = $actualHash; aggregateSha256 = $expectedSourceHash } } |
    ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $bindingPath -Encoding UTF8
  $binding = Test-FootprintCandidateBinding $fakeExecutable $actualHash $expectedSourceHash $bindingPath
  Assert-Equal $actualHash $binding.executableSha256 "Executable binding failed."
  Assert-Throws {
    Test-FootprintCandidateBinding $fakeExecutable ("c" * 64) $expectedSourceHash $bindingPath
  } "A relabeled candidate executable passed."
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

$parseFiles = @($harnessFiles | Where-Object Extension -in @(".ps1", ".psm1"))
foreach ($file in $parseFiles) {
  $tokens = $null
  $errors = $null
  [void][Management.Automation.Language.Parser]::ParseFile($file.FullName, [ref]$tokens, [ref]$errors)
  Assert-Equal 0 $errors.Count "PowerShell parser errors in $($file.Name)."
}

Write-Host "footprint harness tests: passed ($script:CheckCount checks)"
