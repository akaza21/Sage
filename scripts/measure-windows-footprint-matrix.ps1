[CmdletBinding()]
param(
  [Parameter(Mandatory)]
  [string]$ExecutablePath,

  [Parameter(Mandatory)]
  [ValidatePattern("^[0-9a-fA-F]{64}$")]
  [string]$ExpectedExecutableSha256,

  [Parameter(Mandatory)]
  [ValidatePattern("^[0-9a-fA-F]{64}$")]
  [string]$ExpectedSourceAggregateSha256,

  [Parameter(Mandatory)]
  [string]$CandidateBindingPath,

  [Parameter(Mandatory)]
  [string]$SourceManifestPath,

  [Parameter(Mandatory)]
  [string]$FixtureProjectRoot,

  [AllowEmptyString()]
  [string]$DiagnosticDataDirectory = "",

  [string]$OutputDirectory = "",

  [ValidateRange(1, 100)]
  [int]$Launches = 5,

  [ValidateRange(1, 60)]
  [int]$WarmupSeconds = 8,

  [ValidateRange(2, 60)]
  [int]$SampleCount = 5,

  [ValidateRange(100, 10000)]
  [int]$SampleIntervalMs = 1000,

  [string]$NodePath = "node.exe",

  [switch]$Acceptance
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
Import-Module (Join-Path $PSScriptRoot "footprint/ScenarioEvidence.psm1") -Force

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
if (-not [string]::IsNullOrWhiteSpace($DiagnosticDataDirectory) -and
    (-not [IO.Path]::IsPathRooted($DiagnosticDataDirectory) -or
     -not (Test-Path -LiteralPath $DiagnosticDataDirectory -PathType Container))) {
  throw "DiagnosticDataDirectory must identify an existing absolute campaign directory."
}

$measure = Join-Path $PSScriptRoot "measure-windows-footprint.ps1"
$runner = (Resolve-Path (Join-Path $PSScriptRoot "footprint/bounded-node-runner.mjs")).Path
$resolvedNode = (Get-Command $NodePath -CommandType Application -ErrorAction Stop).Source
$resolvedPowerShell = (Get-Command powershell.exe -CommandType Application -ErrorAction Stop).Source
$scenarios = @("chat", "explorer")

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
    "-ExpectedExecutableSha256",
    $ExpectedExecutableSha256,
    "-ExpectedSourceAggregateSha256",
    $ExpectedSourceAggregateSha256,
    "-CandidateBindingPath",
    $CandidateBindingPath,
    "-SourceManifestPath",
    $SourceManifestPath,
    "-FixtureProjectRoot",
    $FixtureProjectRoot,
    "-Launches",
    $Launches,
    "-WarmupSeconds",
    $WarmupSeconds,
    "-SampleCount",
    $SampleCount,
    "-SampleIntervalMs",
    $SampleIntervalMs,
    "-OutputDirectory",
    $scenarioOutput
  )
  if (-not [string]::IsNullOrWhiteSpace($DiagnosticDataDirectory)) {
    $scenarioDataDirectory = Join-Path $DiagnosticDataDirectory $scenario
    if (Test-Path -LiteralPath $scenarioDataDirectory) {
      throw "Scenario diagnostics directory '$scenario' already exists; measurement requires fresh disjoint state."
    }
    [void](New-Item -ItemType Directory -Path $scenarioDataDirectory)
    $arguments += @("-DiagnosticDataDirectory", $scenarioDataDirectory)
  }
  if ($Acceptance) {
    $arguments += "-Acceptance"
  }
  Write-Host "footprint matrix: starting $scenario"
  $scenarioLog = Join-Path $scenarioOutput "campaign-console.log"
  [void](New-Item -ItemType Directory -Force -Path $scenarioOutput)
  try {
    $child = Invoke-FootprintBoundedProcess $resolvedNode $runner $resolvedPowerShell $arguments `
      (Join-Path $scenarioOutput "matrix-child-process.json") 120000 1MB
    @($child.stdout, $child.stderr) | Set-Content -LiteralPath $scenarioLog -Encoding UTF8
  } catch {
    $_.Exception.Message | Set-Content -LiteralPath $scenarioLog -Encoding UTF8
    throw "Footprint scenario '$scenario' failed: $($_.Exception.Message)"
  }
  Write-Host "footprint matrix: completed $scenario"
}

Write-Host "footprint matrix: all scenarios completed"
