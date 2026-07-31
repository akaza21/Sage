[CmdletBinding()]
param(
  [Parameter(Mandatory)]
  [string]$ExecutablePath,

  [ValidateSet(
    "control",
    "control-static",
    "control-geometry",
    "control-scroll",
    "control-layered",
    "shell",
    "chat",
    "file",
    "diff",
    "explorer",
    "source-control"
  )]
  [string]$Scenario = "chat",

  [ValidateRange(1, 100)]
  [int]$Launches = 5,

  [ValidateRange(1, 60)]
  [int]$WarmupSeconds = 8,

  [ValidateRange(2, 60)]
  [int]$SampleCount = 5,

  [ValidateRange(100, 10000)]
  [int]$SampleIntervalMs = 1000,

  [ValidateRange(1, 120)]
  [int]$WindowTimeoutSeconds = 20,

  [ValidateRange(1, 100)]
  [int]$InteractionIterations = 20,

  [ValidateRange(0, 100)]
  [int]$MaxLaunchAttempts = 0,

  [string]$OutputDirectory = "",

  [switch]$MeasureInteractions,

  [switch]$Acceptance
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

Import-Module (
  Join-Path $PSScriptRoot "footprint/WindowsFootprint.psm1"
) -Force

if (-not [IO.Path]::IsPathRooted($ExecutablePath)) {
  throw "ExecutablePath must be an absolute path."
}
if (-not (Test-Path -LiteralPath $ExecutablePath -PathType Leaf)) {
  throw "ExecutablePath does not exist: $ExecutablePath"
}
$resolvedExecutable = (Resolve-Path -LiteralPath $ExecutablePath).Path
if ([IO.Path]::GetExtension($resolvedExecutable) -ne ".exe") {
  throw "ExecutablePath must identify a Windows executable."
}

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
  $workspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
  $stamp = [DateTime]::UtcNow.ToString("yyyyMMdd-HHmmss")
  $OutputDirectory = Join-Path $workspaceRoot (
    "local-evidence/windows-footprint/$stamp-$Scenario"
  )
} elseif (-not [IO.Path]::IsPathRooted($OutputDirectory)) {
  throw "OutputDirectory must be an absolute path."
}
if ($Acceptance -and $Launches -lt 5) {
  throw "Acceptance campaigns require at least five clean launches."
}

$debugEnvironmentNames = @(
  "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
  "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER"
)
$savedEnvironment = @{}
foreach ($name in $debugEnvironmentNames) {
  $value = [Environment]::GetEnvironmentVariable($name, "Process")
  $savedEnvironment[$name] = $value
  if ($Acceptance -and -not [string]::IsNullOrWhiteSpace($value)) {
    throw "Acceptance mode refuses the inherited $name variable."
  }
}

$existing = @(
  Get-CimInstance Win32_Process |
    Where-Object {
      $_.ExecutablePath -and
      [string]::Equals(
        [IO.Path]::GetFullPath([string]$_.ExecutablePath),
        $resolvedExecutable,
        [StringComparison]::OrdinalIgnoreCase
      )
    }
)
if ($existing.Count -gt 0) {
  $ids = ($existing | ForEach-Object ProcessId) -join ", "
  throw "The target executable is already running (PID $ids). Close it first."
}

[void](New-Item -ItemType Directory -Force -Path $OutputDirectory)
$resolvedOutput = (Resolve-Path -LiteralPath $OutputDirectory).Path
Write-Host "footprint: local raw output $resolvedOutput"
$launchResults = @()
$campaignTimer = [Diagnostics.Stopwatch]::StartNew()
$maximumAttempts = if ($MaxLaunchAttempts -eq 0) {
  $Launches + 5
} else {
  $MaxLaunchAttempts
}
if ($maximumAttempts -lt $Launches) {
  throw "MaxLaunchAttempts cannot be less than Launches."
}

try {
  foreach ($name in $debugEnvironmentNames) {
    [Environment]::SetEnvironmentVariable($name, $null, "Process")
  }

  $attempt = 0
  $validLaunchCount = 0
  while (
    $validLaunchCount -lt $Launches -and
    $attempt -lt $maximumAttempts
  ) {
    $attempt += 1
    $launchIndex = $validLaunchCount + 1
    Write-Host (
      "footprint: launch {0}/{1}, attempt {2}/{3} ({4})" -f
      $launchIndex,
      $Launches,
      $attempt,
      $maximumAttempts,
      $Scenario
    )

    $process = $null
    $rootIdentity = $null
    $knownProcessIds = [Collections.Generic.HashSet[int]]::new()
    $launch = [ordered]@{
      launch = $launchIndex
      attempt = $attempt
      startedAtUtc = [DateTime]::UtcNow.ToString("o")
      startupWindowMs = $null
      webBounds = $null
      scenarioSetup = $null
      samples = @()
      idleCpuPercentOneCore = $null
      interaction = $null
      processIdsSeen = @()
      roleSet = @()
      valid = $false
      error = $null
      errorTrace = $null
      teardown = $null
    }

    try {
      $process = Start-Process `
        -FilePath $resolvedExecutable `
        -WorkingDirectory (Split-Path -Parent $resolvedExecutable) `
        -PassThru
      [void]$knownProcessIds.Add($process.Id)
      for ($identityAttempt = 1; $identityAttempt -le 20; $identityAttempt += 1) {
        try {
          $rootIdentity = Get-FootprintRootIdentity `
            -RootProcessId $process.Id `
            -ExpectedExecutablePath $resolvedExecutable
          break
        } catch {
          if ($identityAttempt -eq 20) {
            throw
          }
          Start-Sleep -Milliseconds 50
        }
      }

      $window = Wait-FootprintWindow `
        -ProcessId $process.Id `
        -TimeoutSeconds $WindowTimeoutSeconds
      $launch.startupWindowMs = $window.elapsedMs
      $bounds = Get-FootprintWebBounds -WindowHandle $window.handle
      $launch.webBounds = $bounds

      # Allow the native bootstrap and first renderer navigation to finish
      # before selecting the representative state.
      Start-Sleep -Milliseconds 500
      $launch.scenarioSetup = Invoke-FootprintScenario `
        -Scenario $Scenario `
        -WindowHandle $window.handle `
        -Bounds $bounds
      Start-Sleep -Seconds $WarmupSeconds

      for ($sampleIndex = 1; $sampleIndex -le $SampleCount; $sampleIndex += 1) {
        $sample = Get-FootprintTreeSample -RootProcessId $process.Id
        foreach ($record in $sample.processes) {
          [void]$knownProcessIds.Add([int]$record.processId)
        }
        $launch.samples += $sample
        if ($sampleIndex -lt $SampleCount) {
          Start-Sleep -Milliseconds $SampleIntervalMs
        }
      }

      $firstSample = $launch.samples[0]
      $lastSample = $launch.samples[$launch.samples.Count - 1]
      $wallSeconds = (($launch.samples.Count - 1) * $SampleIntervalMs) / 1000
      $cpuDelta = [Math]::Max(
        0,
        [double]$lastSample.coreCpuSeconds - [double]$firstSample.coreCpuSeconds
      )
      $launch.idleCpuPercentOneCore = if ($wallSeconds -gt 0) {
        [Math]::Round(($cpuDelta / $wallSeconds) * 100, 6)
      } else {
        0.0
      }

      $unknown = @(
        $launch.samples |
          ForEach-Object processes |
          Where-Object { $_.role -in @("unknown", "webview-unknown") }
      )
      $remoteDebug = @(
        $launch.samples |
          ForEach-Object processes |
          Where-Object {
            $_.commandLine -and
            $_.commandLine -match "--remote-debugging-(?:port|pipe)"
          }
      )
      if ($unknown.Count -gt 0) {
        throw "Unclassified descendants were present in the acceptance tree."
      }
      if ($Acceptance -and $remoteDebug.Count -gt 0) {
        throw "A WebView2 debugging endpoint was present in acceptance mode."
      }

      $screenshotPath = Join-Path $resolvedOutput (
        "launch-{0:D2}-attempt-{1:D2}-{2}.png" -f
        $launchIndex,
        $attempt,
        $Scenario
      )
      [SageFootprint.NativeMethods]::BringToFront($window.handle)
      Save-FootprintScreenshot -Bounds $bounds -Path $screenshotPath

      if ($MeasureInteractions -and $Scenario -ne "control") {
        $launch.interaction = Invoke-FootprintInteractions `
          -WindowHandle $window.handle `
          -Bounds $bounds `
          -Iterations $InteractionIterations
      }

      $launch.processIdsSeen = @($knownProcessIds | Sort-Object)
      $launch.roleSet = @(
        $launch.samples |
          ForEach-Object processes |
          ForEach-Object role |
          Sort-Object -Unique
      )
      $launch.valid = $true
    } catch {
      $launch.error = $_.Exception.Message
      $launch.errorTrace = $_.ScriptStackTrace
      Write-Warning (
        "Launch {0} failed: {1}`n{2}" -f
        $launchIndex,
        $launch.error,
        $launch.errorTrace
      )
    } finally {
      if ($null -ne $rootIdentity) {
        $launch.teardown = Stop-FootprintTree `
          -RootIdentity $rootIdentity `
          -KnownProcessIds $knownProcessIds
      } elseif ($null -ne $process -and $process.MainWindowHandle -ne [IntPtr]::Zero) {
        [void]$process.CloseMainWindow()
        $launch.teardown = [ordered]@{
          forced = $false
          identityLost = $true
          candidates = @()
          remaining = @()
        }
      }
      $launch.finishedAtUtc = [DateTime]::UtcNow.ToString("o")
      $launchResults += $launch
    }

    if ($launch.valid) {
      $validLaunchCount += 1
    } else {
      $failedPath = Join-Path $resolvedOutput (
        "failed-attempt-{0:D2}.json" -f $attempt
      )
      $launch | ConvertTo-Json -Depth 12 |
        Set-Content -LiteralPath $failedPath -Encoding UTF8
    }
  }

  if ($validLaunchCount -lt $Launches) {
    throw (
      "Only $validLaunchCount of $Launches required launches succeeded " +
      "within $maximumAttempts attempts."
    )
  }
} finally {
  foreach ($name in $debugEnvironmentNames) {
    [Environment]::SetEnvironmentVariable(
      $name,
      $savedEnvironment[$name],
      "Process"
    )
  }
}

$validLaunches = @($launchResults | Where-Object valid)
$privatePerLaunch = @(
  $validLaunches |
    ForEach-Object {
      Get-FootprintMedian -Values @(
        $_.samples | ForEach-Object { [double]$_.corePrivateBytes }
      )
    }
)
$workingSetPerLaunch = @(
  $validLaunches |
    ForEach-Object {
      Get-FootprintMedian -Values @(
        $_.samples | ForEach-Object { [double]$_.coreWorkingSetBytes }
      )
    }
)
$cpuPerLaunch = @(
  $validLaunches | ForEach-Object { [double]$_.idleCpuPercentOneCore }
)
$startupPerLaunch = @(
  $validLaunches | ForEach-Object { [double]$_.startupWindowMs }
)
$inputValues = @(
  $validLaunches |
    Where-Object { $null -ne $_.interaction } |
    ForEach-Object { $_.interaction.inputToPaintMs }
)
$tabValues = @(
  $validLaunches |
    Where-Object { $null -ne $_.interaction } |
    ForEach-Object { $_.interaction.tabSwitchMs }
)
$roles = @(
  $validLaunches |
    ForEach-Object samples |
    ForEach-Object processes |
    ForEach-Object role |
    Sort-Object -Unique
)
$roleStatistics = [ordered]@{}
foreach ($role in $roles) {
  $rolePrivatePerLaunch = @()
  $roleWorkingSetPerLaunch = @()
  foreach ($launch in $validLaunches) {
    $privateSamples = @()
    $workingSetSamples = @()
    foreach ($sample in $launch.samples) {
      $roleProcesses = @($sample.processes | Where-Object { $_.role -eq $role })
      $privateSamples += [double](
        ($roleProcesses | ForEach-Object privateBytes | Measure-Object -Sum).Sum
      )
      $workingSetSamples += [double](
        ($roleProcesses | ForEach-Object workingSetBytes | Measure-Object -Sum).Sum
      )
    }
    $rolePrivatePerLaunch += Get-FootprintMedian $privateSamples
    $roleWorkingSetPerLaunch += Get-FootprintMedian $workingSetSamples
  }
  $roleStatistics[$role] = [ordered]@{
    privateBytes = [ordered]@{
      perLaunchMedian = $rolePrivatePerLaunch
      median = Get-FootprintMedian $rolePrivatePerLaunch
      p95 = Get-FootprintPercentile $rolePrivatePerLaunch 0.95
    }
    workingSetBytes = [ordered]@{
      perLaunchMedian = $roleWorkingSetPerLaunch
      median = Get-FootprintMedian $roleWorkingSetPerLaunch
      p95 = Get-FootprintPercentile $roleWorkingSetPerLaunch 0.95
    }
  }
}
$webViewPaths = @(
  $validLaunches |
    ForEach-Object samples |
    ForEach-Object processes |
    Where-Object { $_.name -eq "msedgewebview2.exe" -and $_.executablePath } |
    ForEach-Object executablePath |
    Sort-Object -Unique
)
$webViewVersions = @(
  foreach ($path in $webViewPaths) {
    $item = Get-Item -LiteralPath $path -ErrorAction SilentlyContinue
    [ordered]@{
      path = $path
      fileVersion = if ($null -ne $item) {
        $item.VersionInfo.FileVersion
      } else {
        $null
      }
      productVersion = if ($null -ne $item) {
        $item.VersionInfo.ProductVersion
      } else {
        $null
      }
    }
  }
)

$summary = [ordered]@{
  schemaVersion = 1
  generatedAtUtc = [DateTime]::UtcNow.ToString("o")
  executablePath = $resolvedExecutable
  executableBytes = (Get-Item -LiteralPath $resolvedExecutable).Length
  executableSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $resolvedExecutable).Hash
  scenario = $Scenario
  acceptance = [bool]$Acceptance
  validLaunches = $validLaunches.Count
  requestedLaunches = $Launches
  launchAttempts = $launchResults.Count
  maximumLaunchAttempts = $maximumAttempts
  warmupSeconds = $WarmupSeconds
  sampleCount = $SampleCount
  sampleIntervalMs = $SampleIntervalMs
  processBoundary = "launched root plus descendants by Windows parent PID"
  primaryMetric = "summed core private bytes"
  coreRoles = @(
    "native-host",
    "browser",
    "renderer",
    "gpu",
    "utility-network",
    "utility-storage",
    "utility-other",
    "crashpad"
  )
  excludedRoles = @("excluded-child")
  privateBytes = [ordered]@{
    perLaunchMedian = $privatePerLaunch
    median = Get-FootprintMedian $privatePerLaunch
    p95 = Get-FootprintPercentile $privatePerLaunch 0.95
  }
  workingSetBytes = [ordered]@{
    perLaunchMedian = $workingSetPerLaunch
    median = Get-FootprintMedian $workingSetPerLaunch
    p95 = Get-FootprintPercentile $workingSetPerLaunch 0.95
  }
  idleCpuPercentOneCore = [ordered]@{
    perLaunch = $cpuPerLaunch
    median = Get-FootprintMedian $cpuPerLaunch
    p95 = Get-FootprintPercentile $cpuPerLaunch 0.95
  }
  startupWindowMs = [ordered]@{
    perLaunch = $startupPerLaunch
    median = Get-FootprintMedian $startupPerLaunch
    p95 = Get-FootprintPercentile $startupPerLaunch 0.95
  }
  inputToPaintMs = if ($inputValues.Count -gt 0) {
    [ordered]@{
      samples = $inputValues.Count
      median = Get-FootprintMedian $inputValues
      p95 = Get-FootprintPercentile $inputValues 0.95
    }
  } else {
    $null
  }
  tabSwitchMs = if ($tabValues.Count -gt 0) {
    [ordered]@{
      samples = $tabValues.Count
      median = Get-FootprintMedian $tabValues
      p95 = Get-FootprintPercentile $tabValues 0.95
    }
  } else {
    $null
  }
  roleStatistics = $roleStatistics
  webView2 = $webViewVersions
  roleSets = @($validLaunches | ForEach-Object roleSet)
  elapsedSeconds = [Math]::Round($campaignTimer.Elapsed.TotalSeconds, 3)
}

$operatingSystem = Get-CimInstance Win32_OperatingSystem
$raw = [ordered]@{
  schemaVersion = 1
  generatedAtUtc = $summary.generatedAtUtc
  machine = [ordered]@{
    computerName = $env:COMPUTERNAME
    os = $operatingSystem.Caption
    osVersion = [Environment]::OSVersion.Version.ToString()
    logicalProcessors = [Environment]::ProcessorCount
    totalVisibleMemoryBytes = [long]$operatingSystem.TotalVisibleMemorySize * 1KB
    powershell = $PSVersionTable.PSVersion.ToString()
  }
  harness = [ordered]@{
    scriptSha256 = (
      Get-FileHash -Algorithm SHA256 -LiteralPath $PSCommandPath
    ).Hash
    moduleSha256 = (
      Get-FileHash `
        -Algorithm SHA256 `
        -LiteralPath (
          Join-Path $PSScriptRoot "footprint/WindowsFootprint.psm1"
        )
    ).Hash
  }
  configuration = [ordered]@{
    executablePath = $resolvedExecutable
    scenario = $Scenario
    launches = $Launches
    maximumLaunchAttempts = $maximumAttempts
    warmupSeconds = $WarmupSeconds
    sampleCount = $SampleCount
    sampleIntervalMs = $SampleIntervalMs
    acceptance = [bool]$Acceptance
    measureInteractions = [bool]$MeasureInteractions
    interactionIterations = $InteractionIterations
  }
  launches = $launchResults
}

$rawPath = Join-Path $resolvedOutput "raw.json"
$summaryPath = Join-Path $resolvedOutput "summary.json"
$markdownPath = Join-Path $resolvedOutput "summary.md"
$raw | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $rawPath -Encoding UTF8
$summary | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $summaryPath -Encoding UTF8

$privateMiB = [double]$summary.privateBytes.median / 1MB
$privateP95MiB = [double]$summary.privateBytes.p95 / 1MB
$workingSetMiB = [double]$summary.workingSetBytes.median / 1MB
$markdown = @"
# Windows footprint summary

- Executable: ``$resolvedExecutable``
- Scenario: ``$Scenario``
- Clean launches: $($summary.validLaunches)
- Core private bytes median: $([Math]::Round($privateMiB, 3)) MiB
- Core private bytes P95: $([Math]::Round($privateP95MiB, 3)) MiB
- Summed working set median: $([Math]::Round($workingSetMiB, 3)) MiB
- Idle CPU median: $($summary.idleCpuPercentOneCore.median)% of one core
- Launch-to-window P50/P95: $($summary.startupWindowMs.median) / $($summary.startupWindowMs.p95) ms
- Input-to-paint: $(if ($null -eq $summary.inputToPaintMs) { "not measured" } else { "$($summary.inputToPaintMs.median) / $($summary.inputToPaintMs.p95) ms P50/P95" })
- Tab switch: $(if ($null -eq $summary.tabSwitchMs) { "not measured" } else { "$($summary.tabSwitchMs.median) / $($summary.tabSwitchMs.p95) ms P50/P95" })

Private bytes are the primary ownership signal. Summed working set is retained
for observation and may double-count shared WebView2 pages.
"@
$markdown | Set-Content -LiteralPath $markdownPath -Encoding UTF8

Write-Host "footprint: raw $rawPath"
Write-Host "footprint: summary $summaryPath"
$summary | ConvertTo-Json -Depth 8
