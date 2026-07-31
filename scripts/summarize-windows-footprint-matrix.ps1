[CmdletBinding()]
param(
  [Parameter(Mandatory)]
  [string]$ControlDirectory,

  [Parameter(Mandatory)]
  [string]$SageMatrixDirectory,

  [string]$OutputPath = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ([string]::IsNullOrWhiteSpace($OutputPath)) {
  $workspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
  $stamp = [DateTime]::UtcNow.ToString("yyyyMMdd-HHmmss")
  $OutputPath = Join-Path $workspaceRoot (
    "local-evidence/windows-footprint/$stamp-matrix-summary.json"
  )
} elseif (-not [IO.Path]::IsPathRooted($OutputPath)) {
  throw "OutputPath must be absolute."
}

function Read-JsonFile {
  param([Parameter(Mandatory)][string]$Path)

  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    throw "Missing evidence file: $Path"
  }
  return Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
}

function ConvertTo-MiB {
  param([Parameter(Mandatory)][double]$Bytes)

  return [Math]::Round($Bytes / 1MB, 3)
}

function Get-FootprintMedianCompat {
  param([Parameter(Mandatory)][double[]]$Values)

  $ordered = @($Values | Sort-Object)
  $middle = [Math]::Floor($ordered.Count / 2)
  if ($ordered.Count % 2 -eq 1) {
    return [double]$ordered[$middle]
  }
  return ([double]$ordered[$middle - 1] + [double]$ordered[$middle]) / 2
}

$control = Read-JsonFile (Join-Path $ControlDirectory "summary.json")
$scenarios = @("chat", "explorer")
$scenarioResults = @()

foreach ($scenario in $scenarios) {
  $directory = Join-Path $SageMatrixDirectory $scenario
  $summary = Read-JsonFile (Join-Path $directory "summary.json")
  $raw = Read-JsonFile (Join-Path $directory "raw.json")
  $excludedMaximum = @(
    $raw.launches |
      ForEach-Object samples |
      ForEach-Object { [double]$_.excludedPrivateBytes }
  ) | Measure-Object -Maximum
  $processCounts = @(
    $raw.launches |
      ForEach-Object samples |
      ForEach-Object { [double]$_.processCount }
  )
  $forcedTeardowns = @(
    $raw.launches | Where-Object { $_.teardown.forced }
  ).Count

  $scenarioResults += [ordered]@{
    scenario = $scenario
    privateBytesMedian = [double]$summary.privateBytes.median
    privateMiBMedian = ConvertTo-MiB $summary.privateBytes.median
    privateMiBP95 = ConvertTo-MiB $summary.privateBytes.p95
    sageOverControlMiB = ConvertTo-MiB (
      [double]$summary.privateBytes.median -
      [double]$control.privateBytes.median
    )
    workingSetMiBMedian = ConvertTo-MiB $summary.workingSetBytes.median
    idleCpuMedian = [double]$summary.idleCpuPercentOneCore.median
    idleCpuP95 = [double]$summary.idleCpuPercentOneCore.p95
    startupWindowP50Ms = [double]$summary.startupWindowMs.median
    startupWindowP95Ms = [double]$summary.startupWindowMs.p95
    inputToPaintP50Ms = if ($summary.inputToPaintMs) {
      [double]$summary.inputToPaintMs.median
    } else {
      $null
    }
    inputToPaintP95Ms = if ($summary.inputToPaintMs) {
      [double]$summary.inputToPaintMs.p95
    } else {
      $null
    }
    tabSwitchP50Ms = if ($summary.tabSwitchMs) {
      [double]$summary.tabSwitchMs.median
    } else {
      $null
    }
    tabSwitchP95Ms = if ($summary.tabSwitchMs) {
      [double]$summary.tabSwitchMs.p95
    } else {
      $null
    }
    rolePrivateMiB = [ordered]@{
      nativeHost = ConvertTo-MiB (
        $summary.roleStatistics."native-host".privateBytes.median
      )
      browser = ConvertTo-MiB (
        $summary.roleStatistics.browser.privateBytes.median
      )
      renderer = ConvertTo-MiB (
        $summary.roleStatistics.renderer.privateBytes.median
      )
      gpu = ConvertTo-MiB (
        $summary.roleStatistics.gpu.privateBytes.median
      )
      network = ConvertTo-MiB (
        $summary.roleStatistics."utility-network".privateBytes.median
      )
      storage = ConvertTo-MiB (
        $summary.roleStatistics."utility-storage".privateBytes.median
      )
      crashpad = ConvertTo-MiB (
        $summary.roleStatistics.crashpad.privateBytes.median
      )
    }
    processCountMedian = [int](
      Get-FootprintMedianCompat $processCounts
    )
    excludedPrivateBytesMaximum = [long]$excludedMaximum.Maximum
    forcedTeardowns = $forcedTeardowns
    webView2 = $summary.webView2
  }
}

$maximumScenario = $scenarioResults |
  Sort-Object privateBytesMedian -Descending |
  Select-Object -First 1
$result = [ordered]@{
  schemaVersion = 1
  generatedAtUtc = [DateTime]::UtcNow.ToString("o")
  control = [ordered]@{
    privateMiBMedian = ConvertTo-MiB $control.privateBytes.median
    privateMiBP95 = ConvertTo-MiB $control.privateBytes.p95
    workingSetMiBMedian = ConvertTo-MiB $control.workingSetBytes.median
    startupWindowP50Ms = [double]$control.startupWindowMs.median
    startupWindowP95Ms = [double]$control.startupWindowMs.p95
    webView2 = $control.webView2
  }
  scenarios = $scenarioResults
  maximumScenario = $maximumScenario.scenario
  maximumPrivateMiBMedian = $maximumScenario.privateMiBMedian
  maximumSageOverControlMiB = $maximumScenario.sageOverControlMiB
}

$parent = Split-Path -Parent $OutputPath
if (-not (Test-Path -LiteralPath $parent)) {
  [void](New-Item -ItemType Directory -Force -Path $parent)
}
$result | ConvertTo-Json -Depth 10 |
  Set-Content -LiteralPath $OutputPath -Encoding UTF8
$result | ConvertTo-Json -Depth 10
