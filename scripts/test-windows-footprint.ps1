$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

Import-Module (
  Join-Path $PSScriptRoot "footprint/WindowsFootprint.psm1"
) -Force

function Assert-Equal {
  param(
    [Parameter(Mandatory)]$Expected,
    [Parameter(Mandatory)]$Actual,
    [Parameter(Mandatory)][string]$Message
  )

  if ($Expected -ne $Actual) {
    throw "$Message Expected '$Expected', got '$Actual'."
  }
}

Assert-Equal 2.5 (Get-FootprintMedian @(4.0, 1.0)) "Even median failed."
Assert-Equal 3.0 (Get-FootprintMedian @(3.0, 1.0, 9.0)) "Odd median failed."
Assert-Equal 5.0 (
  Get-FootprintPercentile @(1.0, 2.0, 3.0, 4.0, 5.0) 0.95
) "Nearest-rank P95 failed."

Assert-Equal "native-host" (
  Get-FootprintProcessRole "sage.exe" $null $true
) "Root classification failed."
Assert-Equal "browser" (
  Get-FootprintProcessRole `
    "msedgewebview2.exe" `
    "msedgewebview2.exe --embedded-browser-webview=1" `
    $false
) "Browser classification failed."
Assert-Equal "renderer" (
  Get-FootprintProcessRole `
    "msedgewebview2.exe" `
    "msedgewebview2.exe --type=renderer" `
    $false
) "Renderer classification failed."
Assert-Equal "gpu" (
  Get-FootprintProcessRole `
    "msedgewebview2.exe" `
    "msedgewebview2.exe --type=gpu-process" `
    $false
) "GPU classification failed."
Assert-Equal "utility-network" (
  Get-FootprintProcessRole `
    "msedgewebview2.exe" `
    "msedgewebview2.exe --type=utility --utility-sub-type=network.mojom.NetworkService" `
    $false
) "Network classification failed."
Assert-Equal "utility-storage" (
  Get-FootprintProcessRole `
    "msedgewebview2.exe" `
    "msedgewebview2.exe --type=utility --utility-sub-type=storage.mojom.StorageService" `
    $false
) "Storage classification failed."
Assert-Equal "crashpad" (
  Get-FootprintProcessRole `
    "msedgewebview2.exe" `
    "msedgewebview2.exe --type=crashpad-handler" `
    $false
) "Crashpad classification failed."
Assert-Equal "excluded-child" (
  Get-FootprintProcessRole "pwsh.exe" "pwsh.exe" $false
) "Excluded child classification failed."
Assert-Equal "unknown" (
  Get-FootprintProcessRole "surprise.exe" "surprise.exe" $false
) "Unknown child classification failed."

$rows = @(
  [pscustomobject]@{ ProcessId = 10; ParentProcessId = 1 },
  [pscustomobject]@{ ProcessId = 11; ParentProcessId = 10 },
  [pscustomobject]@{ ProcessId = 12; ParentProcessId = 11 },
  [pscustomobject]@{ ProcessId = 20; ParentProcessId = 1 }
)
$descendants = @(Get-FootprintDescendants 10 $rows)
Assert-Equal 3 $descendants.Count "Descendant count failed."
Assert-Equal "10,11,12" (
  ($descendants | ForEach-Object ProcessId) -join ","
) "Descendant membership failed."

$rootIdentity = [ordered]@{
  processId = 10
  executablePath = "C:\Apps\sage.exe"
  creationDate = "2026-07-30T00:00:00.0000000Z"
}
$teardownRows = @(
  [pscustomobject]@{
    ProcessId = 10
    ParentProcessId = 1
    ExecutablePath = "C:\Apps\sage.exe"
    CreationDate = "2026-07-30T00:00:00.0000000Z"
  },
  [pscustomobject]@{
    ProcessId = 11
    ParentProcessId = 10
    ExecutablePath = "C:\Apps\msedgewebview2.exe"
    CreationDate = "2026-07-30T00:00:01.0000000Z"
  },
  # PID 12 was remembered from an earlier sample but has exited and been
  # reused by a process that is no longer in the launched root's ancestry.
  [pscustomobject]@{
    ProcessId = 12
    ParentProcessId = 1
    ExecutablePath = "C:\Windows\unrelated.exe"
    CreationDate = "2026-07-30T00:00:09.0000000Z"
  }
)
$forceCandidates = @(
  Get-FootprintForceKillCandidates `
    -RootIdentity $rootIdentity `
    -ProcessRows $teardownRows
)
Assert-Equal "10,11" (
  ($forceCandidates | ForEach-Object ProcessId) -join ","
) "Reused remembered PID was not excluded from force-kill candidates."

$reusedRootRows = @(
  [pscustomobject]@{
    ProcessId = 10
    ParentProcessId = 1
    ExecutablePath = "C:\Apps\sage.exe"
    CreationDate = "2026-07-30T00:01:00.0000000Z"
  },
  [pscustomobject]@{
    ProcessId = 11
    ParentProcessId = 10
    ExecutablePath = "C:\Apps\msedgewebview2.exe"
    CreationDate = "2026-07-30T00:01:01.0000000Z"
  }
)
Assert-Equal 0 @(
  Get-FootprintForceKillCandidates `
    -RootIdentity $rootIdentity `
    -ProcessRows $reusedRootRows
).Count "Reused root PID was not rejected by creation identity."

$bounds = [ordered]@{ x = 100; y = 200; width = 1725; height = 1055 }
$point = ConvertTo-FootprintScreenPoint $bounds 248 42 1380
Assert-Equal 410 $point.x "DPI-scaled X coordinate failed."
Assert-Equal 253 $point.y "DPI-scaled Y coordinate failed."

Write-Host "footprint harness tests: passed"
