[CmdletBinding()]
param(
  [Parameter(Mandatory)]
  [string]$ExecutablePath,

  [string]$OutputDirectory = "",

  [ValidateRange(5, 20)]
  [int]$Launches = 5,

  [ValidateRange(1, 60)]
  [int]$WarmupSeconds = 8,

  [ValidateRange(2, 30)]
  [int]$SampleCount = 5,

  [ValidateRange(100, 5000)]
  [int]$SampleIntervalMs = 1000
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if (-not [IO.Path]::IsPathRooted($ExecutablePath)) {
  throw "ExecutablePath must be absolute."
}
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
  $workspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
  $stamp = [DateTime]::UtcNow.ToString("yyyyMMdd-HHmmss")
  $OutputDirectory = Join-Path $workspaceRoot (
    "local-evidence/windows-footprint/$stamp-gpu-attribution"
  )
} elseif (-not [IO.Path]::IsPathRooted($OutputDirectory)) {
  throw "OutputDirectory must be absolute."
}
Write-Host "gpu attribution: local raw output $OutputDirectory"

$measureScript = Join-Path $PSScriptRoot "measure-windows-footprint.ps1"
$scenarios = @(
  "control-static",
  "control-geometry",
  "control-scroll",
  "control-layered"
)

foreach ($scenario in $scenarios) {
  $scenarioOutput = Join-Path $OutputDirectory $scenario
  Write-Host "gpu attribution: starting $scenario"

  & powershell.exe `
    -NoProfile `
    -ExecutionPolicy Bypass `
    -File $measureScript `
    -ExecutablePath $ExecutablePath `
    -Scenario $scenario `
    -Launches $Launches `
    -WarmupSeconds $WarmupSeconds `
    -SampleCount $SampleCount `
    -SampleIntervalMs $SampleIntervalMs `
    -WindowTimeoutSeconds 20 `
    -MaxLaunchAttempts ($Launches + 5) `
    -OutputDirectory $scenarioOutput `
    -Acceptance

  if ($LASTEXITCODE -ne 0) {
    throw "$scenario measurement failed with exit code $LASTEXITCODE."
  }

  Write-Host "gpu attribution: completed $scenario"
}

Write-Host "gpu attribution: all scenarios completed"
