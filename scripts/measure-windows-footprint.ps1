[CmdletBinding()]
param(
  [Parameter(Mandatory)][string]$ExecutablePath,
  [Parameter(Mandatory)][ValidateSet("chat", "explorer")][string]$Scenario,
  [Parameter(Mandatory)][ValidatePattern("^[0-9a-fA-F]{64}$")][string]$ExpectedExecutableSha256,
  [Parameter(Mandatory)][ValidatePattern("^[0-9a-fA-F]{64}$")][string]$ExpectedSourceAggregateSha256,
  [Parameter(Mandatory)][string]$CandidateBindingPath,
  [Parameter(Mandatory)][string]$SourceManifestPath,
  [ValidateRange(1, 100)][int]$Launches = 5,
  [ValidateRange(1, 60)][int]$WarmupSeconds = 8,
  [ValidateRange(2, 60)][int]$SampleCount = 5,
  [ValidateRange(100, 10000)][int]$SampleIntervalMs = 1000,
  [ValidateRange(1, 120)][int]$WindowTimeoutSeconds = 20,
  [ValidateRange(0, 100)][int]$MaxLaunchAttempts = 0,
  [string]$NodePath = "node.exe",
  [string]$OutputDirectory = "",
  [switch]$Acceptance
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

Import-Module (Join-Path $PSScriptRoot "footprint/WindowsFootprint.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "footprint/ProcessIdentity.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "footprint/ScenarioEvidence.psm1") -Force

if (-not [IO.Path]::IsPathRooted($ExecutablePath) -or -not (Test-Path -LiteralPath $ExecutablePath -PathType Leaf)) {
  throw "ExecutablePath must identify an existing absolute file."
}
$resolvedExecutable = (Resolve-Path -LiteralPath $ExecutablePath).Path
if ([IO.Path]::GetExtension($resolvedExecutable) -ne ".exe") { throw "ExecutablePath must identify a Windows executable." }
if ($Acceptance -and ($Launches -ne 5 -or $WarmupSeconds -ne 8 -or $SampleCount -ne 5 -or $SampleIntervalMs -ne 1000)) {
  throw "Acceptance requires five launches, eight-second warm-up, and five one-second samples."
}
if ($Acceptance -and $MaxLaunchAttempts -gt 10) {
  throw "Acceptance allows at most ten attempts to obtain five valid launches."
}
$nodeCommand = Get-Command $NodePath -CommandType Application -ErrorAction Stop
$resolvedNode = $nodeCommand.Source
$binding = Test-FootprintCandidateBinding $resolvedExecutable $ExpectedExecutableSha256 $ExpectedSourceAggregateSha256 $CandidateBindingPath $SourceManifestPath
Assert-FootprintDebugStateClear

$existing = @(Get-FootprintProcessSnapshot | Where-Object {
  $_.ExecutablePath -and [string]::Equals([IO.Path]::GetFullPath([string]$_.ExecutablePath), $resolvedExecutable, [StringComparison]::OrdinalIgnoreCase)
})
if ($existing.Count -gt 0) { throw "The exact candidate is already running; acceptance requires a clean process boundary." }

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
  $workspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
  $OutputDirectory = Join-Path $workspaceRoot ("local-evidence/windows-footprint/{0}-{1}" -f [DateTime]::UtcNow.ToString("yyyyMMdd-HHmmss"), $Scenario)
} elseif (-not [IO.Path]::IsPathRooted($OutputDirectory)) {
  throw "OutputDirectory must be absolute."
}
[void](New-Item -ItemType Directory -Force -Path $OutputDirectory)
$resolvedOutput = (Resolve-Path -LiteralPath $OutputDirectory).Path
$setupHelper = (Resolve-Path (Join-Path $PSScriptRoot "footprint/prepare-sage-scenario.mjs")).Path
$debugNames = @("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER")
$campaignTimer = [Diagnostics.Stopwatch]::StartNew()

function Get-AvailableLoopbackPort {
  $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
  try { $listener.Start(); return ([Net.IPEndPoint]$listener.LocalEndpoint).Port } finally { $listener.Stop() }
}

function Start-ExactFootprintProcess {
  [void](Assert-FootprintExecutableHash $resolvedExecutable $binding.executableSha256)
  $launched = Start-Process -FilePath $resolvedExecutable -WorkingDirectory (Split-Path -Parent $resolvedExecutable) -PassThru
  try {
    [void](Assert-FootprintExecutableHash $resolvedExecutable $binding.executableSha256)
  } catch {
    $launchFailure = $_
    try {
      $cleanup = Stop-FootprintLaunchedProcessObject $launched $resolvedExecutable
      if (-not $cleanup.exited -or -not $cleanup.treeBoundaryVerified) {
        throw "Post-launch validation cleanup did not verify the exact process tree."
      }
    } catch {
      throw "Post-launch candidate validation failed and exact process-object cleanup could not be verified: $($_.Exception.Message)"
    }
    throw $launchFailure
  }
  return $launched
}

function Wait-ExactDebugListener {
  param(
    [Parameter(Mandatory)][int]$Port,
    [Parameter(Mandatory)][int[]]$AllowedProcessIds,
    [ValidateRange(1, 30)][int]$TimeoutSeconds = 10
  )
  $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  do {
    $listeners = @(Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction SilentlyContinue)
    if ($listeners.Count -gt 0) {
      $owners = @($listeners | ForEach-Object OwningProcess | Sort-Object -Unique)
      $foreign = @($owners | Where-Object { $_ -notin $AllowedProcessIds })
      if ($foreign.Count -gt 0) { throw "Setup debug endpoint has a foreign owning PID." }
      return $owners
    }
    Start-Sleep -Milliseconds 100
  } while ([DateTime]::UtcNow -lt $deadline)
  throw "Setup debug endpoint did not bind to the exact candidate tree."
}

function Get-ExactRootIdentity {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$LaunchIdentity,
    [Parameter(Mandatory)][Collections.Generic.Dictionary[string, object]]$KnownProcessIdentities
  )
  for ($attempt = 1; $attempt -le 20; $attempt += 1) {
    try {
      $rows = @(Get-FootprintProcessSnapshot)
      $rootRows = @($rows | Where-Object { [int]$_.ProcessId -eq [int]$LaunchIdentity.processId })
      if ($rootRows.Count -eq 0 -or @($rootRows | Where-Object {
        $_.ExecutablePath -and $_.CreationDate -and
        [string]::Equals([IO.Path]::GetFullPath([string]$_.ExecutablePath), $LaunchIdentity.executablePath, [StringComparison]::OrdinalIgnoreCase) -and
        (ConvertTo-FootprintProcessIdentity $_).creationDate -eq $LaunchIdentity.creationDate
      }).Count -eq 1) {
        foreach ($row in @(Get-FootprintDescendants ([int]$LaunchIdentity.processId) $rows)) {
          [void](Add-FootprintRememberedIdentity $KnownProcessIdentities $row)
        }
      }
      return Get-FootprintRootIdentity $LaunchIdentity.processId $resolvedExecutable $LaunchIdentity.creationDate $rows
    } catch {
      if ($attempt -eq 20) { throw }
      Start-Sleep -Milliseconds 50
    }
  }
}

function Invoke-InstrumentedScenarioSetup {
  $port = Get-AvailableLoopbackPort
  $output = Join-Path $resolvedOutput "setup-semantic.json"
  if (Test-Path -LiteralPath $output) { throw "Setup output already exists; use a fresh evidence directory." }
  $process = $null
  $identity = $null
  $known = New-FootprintIdentityStore
  try {
    try {
      [Environment]::SetEnvironmentVariable("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--remote-debugging-port=$port", "Process")
      [Environment]::SetEnvironmentVariable("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", $null, "Process")
      $process = Start-ExactFootprintProcess
      $identity = Get-FootprintLaunchIdentity $process $resolvedExecutable
      [void](Add-FootprintRememberedIdentity $known ([pscustomobject]@{ ProcessId = $identity.processId; ExecutablePath = $identity.executablePath; CreationDate = $identity.creationDate }))
      $verifiedIdentity = Get-ExactRootIdentity $identity $known
      $identity = $verifiedIdentity
    } finally {
      foreach ($name in $debugNames) { [Environment]::SetEnvironmentVariable($name, $null, "Process") }
    }
    $window = Wait-FootprintWindow $process.Id $WindowTimeoutSeconds
    Start-Sleep -Milliseconds 500
    $tree = Get-FootprintTreeSample $process.Id
    foreach ($record in $tree.processes) { [void](Add-FootprintRememberedIdentity $known $record) }
    if ($tree.unknownProcessCount -ne 0) { throw "Setup process tree contains an unknown descendant." }
    $debugRows = @($tree.processes | Where-Object { $_.commandLine -and $_.commandLine -match "--remote-debugging-port=$port(?:\s|$)" })
    if ($debugRows.Count -lt 1) { throw "Setup endpoint was not bound to the exact candidate process tree." }
    $treeProcessIds = @($tree.processes | ForEach-Object processId)
    $debugOwnerProcessIds = @(Wait-ExactDebugListener $port $treeProcessIds)
    & $resolvedNode $setupHelper $port $Scenario $output
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $output -PathType Leaf)) { throw "Semantic CDP setup failed." }
    $semantic = Get-Content -LiteralPath $output -Raw | ConvertFrom-Json
    $verifiedTree = Get-FootprintTreeSample $process.Id
    foreach ($record in $verifiedTree.processes) { [void](Add-FootprintRememberedIdentity $known $record) }
    if ($verifiedTree.unknownProcessCount -ne 0) { throw "Verified setup tree contains an unknown descendant." }
    return [ordered]@{
      scenario = $Scenario
      executed = [bool]$semantic.executed
      semanticVerified = [bool]$semantic.semanticVerified
      processId = $process.Id
      title = $window.title
      origin = [string]$semantic.origin
      executablePath = $resolvedExecutable
      executableSha256 = $binding.executableSha256
      sourceAggregateSha256 = $binding.sourceAggregateSha256
      loopbackPort = $port
      targetId = [string]$semantic.targetId
      debugOwnerProcessIds = $debugOwnerProcessIds
      chatCenterCount = [int]$semantic.chatCenterCount
      explorerSidebarCount = [int]$semantic.explorerSidebarCount
      processTree = $tree
      verifiedProcessTree = $verifiedTree
    }
  } finally {
    if ($null -ne $identity) {
      $teardown = Stop-FootprintTree $identity $known 10
      if ($teardown.identityLost -or $teardown.remaining.Count -ne 0) { throw "Instrumented setup teardown was incomplete or ambiguous." }
    } elseif ($null -ne $process) {
      $teardown = Stop-FootprintLaunchedProcessObject $process $resolvedExecutable $known
      if (-not $teardown.exited -or -not $teardown.treeBoundaryVerified) { throw "Instrumented setup exact process-object fallback cleanup was not verified." }
    }
    Assert-FootprintDebugStateClear
  }
}

$setup = Invoke-InstrumentedScenarioSetup
Assert-FootprintDebugStateClear
$remainingSetupProcesses = @(Get-FootprintProcessSnapshot | Where-Object {
  $_.ExecutablePath -and [string]::Equals([IO.Path]::GetFullPath([string]$_.ExecutablePath), $resolvedExecutable, [StringComparison]::OrdinalIgnoreCase)
})
if ($remainingSetupProcesses.Count -gt 0) { throw "The instrumented setup process tree did not terminate completely." }
$setupPid = [int]$setup.processId
$launchResults = @()
$maximumAttempts = if ($MaxLaunchAttempts -eq 0) { $Launches + 5 } else { $MaxLaunchAttempts }
if ($maximumAttempts -lt $Launches) { throw "MaxLaunchAttempts cannot be less than Launches." }
$attempt = 0
$validCount = 0

while ($validCount -lt $Launches -and $attempt -lt $maximumAttempts) {
  $attempt += 1
  $process = $null
  $identity = $null
  $known = New-FootprintIdentityStore
  $launch = [ordered]@{
    launch = $validCount + 1
    attempt = $attempt
    startedAtUtc = [DateTime]::UtcNow.ToString("o")
    rootIdentity = $null
    startupWindowMs = $null
    scenarioEvidence = $null
    postSampleScenarioEvidence = $null
    samples = @()
    idleCpuPercentOneCore = $null
    processIdsSeen = @()
    roleSet = @()
    valid = $false
    error = $null
    teardown = $null
  }
  try {
    Assert-FootprintDebugStateClear
    $process = Start-ExactFootprintProcess
    if ($process.Id -eq $setupPid) { throw "Measured process reused the setup PID." }
    $identity = Get-FootprintLaunchIdentity $process $resolvedExecutable
    [void](Add-FootprintRememberedIdentity $known ([pscustomobject]@{ ProcessId = $identity.processId; ExecutablePath = $identity.executablePath; CreationDate = $identity.creationDate }))
    $verifiedIdentity = Get-ExactRootIdentity $identity $known
    $identity = $verifiedIdentity
    $launch.rootIdentity = $identity
    $window = Wait-FootprintWindow $process.Id $WindowTimeoutSeconds
    $launch.startupWindowMs = $window.elapsedMs
    $launch.scenarioEvidence = Wait-FootprintScenarioUiaEvidence $window.handle $process.Id $Scenario
    Start-Sleep -Seconds $WarmupSeconds
    for ($sampleIndex = 1; $sampleIndex -le $SampleCount; $sampleIndex += 1) {
      $sample = Get-FootprintTreeSample $process.Id
      foreach ($record in $sample.processes) { [void](Add-FootprintRememberedIdentity $known $record) }
      if ($sample.unknownProcessCount -ne 0) { throw "Measured tree contains an unknown descendant." }
      Assert-FootprintDebugStateClear @($sample.processes)
      $launch.samples += $sample
      if ($sampleIndex -lt $SampleCount) { Start-Sleep -Milliseconds $SampleIntervalMs }
    }
    $first = $launch.samples[0]
    $last = $launch.samples[$launch.samples.Count - 1]
    $wallSeconds = (($launch.samples.Count - 1) * $SampleIntervalMs) / 1000
    $launch.idleCpuPercentOneCore = Get-FootprintCpuPercent $first.coreCpuSeconds $last.coreCpuSeconds $wallSeconds
    $launch.postSampleScenarioEvidence = Wait-FootprintScenarioUiaEvidence $window.handle $process.Id $Scenario
    [void](Assert-FootprintScenarioAcceptance $Scenario $setup $launch.postSampleScenarioEvidence @($launch.samples) $resolvedExecutable $binding.executableSha256 $binding.sourceAggregateSha256)
    $launch.processIdsSeen = @($known.Values | ForEach-Object processId | Sort-Object -Unique)
    $launch.roleSet = @($launch.samples | ForEach-Object processes | ForEach-Object role | Sort-Object -Unique)
    $launch.valid = $true
  } catch {
    $launch.error = $_.Exception.Message
    $launch.errorTrace = $_.ScriptStackTrace
  } finally {
    if ($null -ne $identity) {
      $launch.teardown = Stop-FootprintTree $identity $known 10
      if ($launch.teardown.identityLost -or $launch.teardown.remaining.Count -ne 0) {
        $launch.valid = $false
        $launch.error = "Measured teardown was incomplete or ambiguous."
      }
    } elseif ($null -ne $process) {
      $launch.teardown = Stop-FootprintLaunchedProcessObject $process $resolvedExecutable $known
      if (-not $launch.teardown.exited -or -not $launch.teardown.treeBoundaryVerified) {
        throw "Measured exact process-object fallback cleanup was not verified."
      }
    }
    $launch.finishedAtUtc = [DateTime]::UtcNow.ToString("o")
    $launchResults += $launch
  }
  if ($launch.valid) { $validCount += 1 } else {
    $launch | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $resolvedOutput ("failed-attempt-{0:D2}.json" -f $attempt)) -Encoding UTF8
  }
}
if ($validCount -ne $Launches) { throw "Only $validCount of $Launches required measured launches passed." }

$valid = @($launchResults | Where-Object valid)
$private = @($valid | ForEach-Object { Get-FootprintMedian @($_.samples | ForEach-Object corePrivateBytes) })
$working = @($valid | ForEach-Object { Get-FootprintMedian @($_.samples | ForEach-Object coreWorkingSetBytes) })
$cpu = @($valid | ForEach-Object idleCpuPercentOneCore)
$startup = @($valid | ForEach-Object startupWindowMs)
$roles = @($valid | ForEach-Object samples | ForEach-Object processes | Where-Object coreIncluded | ForEach-Object role | Sort-Object -Unique)
$roleStatistics = [ordered]@{}
foreach ($role in $roles) {
  $rolePrivate = @()
  $roleWorking = @()
  foreach ($launch in $valid) {
    $rolePrivate += Get-FootprintMedian @($launch.samples | ForEach-Object {
      [long](($_.processes | Where-Object role -eq $role | Measure-Object privateBytes -Sum).Sum)
    })
    $roleWorking += Get-FootprintMedian @($launch.samples | ForEach-Object {
      [long](($_.processes | Where-Object role -eq $role | Measure-Object workingSetBytes -Sum).Sum)
    })
  }
  $roleStatistics[$role] = [ordered]@{
    privateBytes = [ordered]@{ perLaunchMedian = $rolePrivate; median = Get-FootprintMedian $rolePrivate; p95 = Get-FootprintPercentile $rolePrivate 0.95 }
    workingSetBytes = [ordered]@{ perLaunchMedian = $roleWorking; median = Get-FootprintMedian $roleWorking; p95 = Get-FootprintPercentile $roleWorking 0.95 }
  }
}
$webViewPaths = @($valid | ForEach-Object samples | ForEach-Object processes | Where-Object { $_.name -eq "msedgewebview2.exe" } | ForEach-Object executablePath | Where-Object { $_ } | Sort-Object -Unique)
$webViewVersions = @($webViewPaths | ForEach-Object {
  $item = Get-Item -LiteralPath $_ -ErrorAction SilentlyContinue
  [ordered]@{
    path = $_
    fileVersion = if ($item) { $item.VersionInfo.FileVersion } else { $null }
    productVersion = if ($item) { $item.VersionInfo.ProductVersion } else { $null }
  }
})
$summary = [ordered]@{
  schemaVersion = 2
  generatedAtUtc = [DateTime]::UtcNow.ToString("o")
  candidate = $binding
  executablePath = $resolvedExecutable
  executableBytes = (Get-Item -LiteralPath $resolvedExecutable).Length
  executableSha256 = $binding.executableSha256
  scenario = $Scenario
  acceptance = [bool]$Acceptance
  setupProcessId = $setupPid
  validLaunches = $valid.Count
  requestedLaunches = $Launches
  launchAttempts = $launchResults.Count
  maximumLaunchAttempts = $maximumAttempts
  warmupSeconds = $WarmupSeconds
  sampleCount = $SampleCount
  sampleIntervalMs = $SampleIntervalMs
  processBoundary = "exact launched root plus classified Windows descendants"
  processLimits = Get-FootprintProcessLimits
  primaryMetric = "summed core private bytes"
  coreRoles = @("native-host", "browser", "renderer", "gpu", "utility-network", "utility-storage", "utility-other", "crashpad")
  excludedRoles = @("excluded-child")
  privateBytes = [ordered]@{ perLaunchMedian = $private; median = Get-FootprintMedian $private; p95 = Get-FootprintPercentile $private 0.95 }
  workingSetBytes = [ordered]@{ perLaunchMedian = $working; median = Get-FootprintMedian $working; p95 = Get-FootprintPercentile $working 0.95 }
  idleCpuPercentOneCore = [ordered]@{ perLaunch = $cpu; median = Get-FootprintMedian $cpu; p95 = Get-FootprintPercentile $cpu 0.95 }
  startupWindowMs = [ordered]@{ perLaunch = $startup; median = Get-FootprintMedian $startup; p95 = Get-FootprintPercentile $startup 0.95 }
  roleStatistics = $roleStatistics
  webView2 = [ordered]@{ executablePaths = $webViewPaths; fileVersions = $webViewVersions }
  roleSets = @($valid | ForEach-Object roleSet)
  elapsedSeconds = [Math]::Round($campaignTimer.Elapsed.TotalSeconds, 3)
}
$operatingSystem = Get-CimInstance Win32_OperatingSystem
$raw = [ordered]@{
  schemaVersion = 2
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
    scriptSha256 = (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash
    processModuleSha256 = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot "footprint/WindowsFootprint.psm1") -Algorithm SHA256).Hash
    processIdentityModuleSha256 = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot "footprint/ProcessIdentity.psm1") -Algorithm SHA256).Hash
    scenarioModuleSha256 = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot "footprint/ScenarioEvidence.psm1") -Algorithm SHA256).Hash
    setupHelperSha256 = (Get-FileHash -LiteralPath $setupHelper -Algorithm SHA256).Hash
  }
  configuration = [ordered]@{
    scenario = $Scenario
    launches = $Launches
    maximumLaunchAttempts = $maximumAttempts
    warmupSeconds = $WarmupSeconds
    sampleCount = $SampleCount
    sampleIntervalMs = $SampleIntervalMs
    acceptance = [bool]$Acceptance
  }
  candidate = $binding
  setup = $setup
  launches = $launchResults
}
$raw | ConvertTo-Json -Depth 14 | Set-Content -LiteralPath (Join-Path $resolvedOutput "raw.json") -Encoding UTF8
$summary | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $resolvedOutput "summary.json") -Encoding UTF8
$markdown = @"
# Windows footprint summary

- Executable SHA-256: ``$($binding.executableSha256)``
- Source aggregate: ``$($binding.sourceAggregateSha256)``
- Scenario: ``$Scenario``
- Setup process: ``$setupPid``; measured launches: $($summary.validLaunches)
- Core private bytes P50/P95: $($summary.privateBytes.median) / $($summary.privateBytes.p95)
- Working set P50/P95: $($summary.workingSetBytes.median) / $($summary.workingSetBytes.p95)
- Idle CPU P50/P95: $($summary.idleCpuPercentOneCore.median)% / $($summary.idleCpuPercentOneCore.p95)% of one core
- Launch-to-window P50/P95: $($summary.startupWindowMs.median) / $($summary.startupWindowMs.p95) ms

The instrumented setup process is excluded from measurement. Every measured
launch is uninstrumented and proves the scenario through exact-PID UI Automation.
"@
$markdown | Set-Content -LiteralPath (Join-Path $resolvedOutput "summary.md") -Encoding UTF8
$summary | ConvertTo-Json -Depth 8
