[CmdletBinding()]
param(
  [Parameter(Mandatory)]
  [string]$ExecutablePath,

  [string]$OutputDirectory = "",

  [ValidateRange(1, 100)]
  [int]$Launches = 5,

  [ValidateRange(1, 60)]
  [int]$WarmupSeconds = 8,

  [ValidateRange(2, 60)]
  [int]$SampleCount = 5,

  [ValidateRange(100, 10000)]
  [int]$SampleIntervalMs = 1000,

  [ValidateRange(1, 100)]
  [int]$InteractionIterations = 20,

  [switch]$Acceptance
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
    "local-evidence/windows-footprint/$stamp-sage-matrix"
  )
} elseif (-not [IO.Path]::IsPathRooted($OutputDirectory)) {
  throw "OutputDirectory must be absolute."
}
Write-Host "footprint matrix: local raw output $OutputDirectory"

$measure = Join-Path $PSScriptRoot "measure-windows-footprint.ps1"
$scenarios = @(
  "shell",
  "chat",
  "file",
  "diff",
  "explorer",
  "source-control"
)

foreach ($scenario in $scenarios) {
  $scenarioOutput = Join-Path $OutputDirectory $scenario
  $arguments = @(
    "-NoProfile",
    "-ExecutionPolicy",
    "Bypass",
    "-File",
    $measure,
    "-ExecutablePath",
    $ExecutablePath,
    "-Scenario",
    $scenario,
    "-Launches",
    $Launches,
    "-WarmupSeconds",
    $WarmupSeconds,
    "-SampleCount",
    $SampleCount,
    "-SampleIntervalMs",
    $SampleIntervalMs,
    "-InteractionIterations",
    $InteractionIterations,
    "-OutputDirectory",
    $scenarioOutput
  )
  if ($Acceptance) {
    $arguments += "-Acceptance"
  }
  if ($scenario -eq "chat") {
    $arguments += "-MeasureInteractions"
  }

  Write-Host "footprint matrix: starting $scenario"
  $scenarioLog = Join-Path $scenarioOutput "campaign-console.log"
  [void](New-Item -ItemType Directory -Force -Path $scenarioOutput)
  & powershell.exe @arguments *> $scenarioLog
  if ($LASTEXITCODE -ne 0) {
    Get-Content -LiteralPath $scenarioLog -ErrorAction SilentlyContinue
    throw "Footprint scenario '$scenario' failed with exit code $LASTEXITCODE."
  }
  Write-Host "footprint matrix: completed $scenario"
}

Write-Host "footprint matrix: all scenarios completed"
