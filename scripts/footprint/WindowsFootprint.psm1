Set-StrictMode -Version Latest
Import-Module (Join-Path $PSScriptRoot "ProcessIdentity.psm1") -Force

function Get-FootprintMedian {
  param([Parameter(Mandatory)][double[]]$Values)
  if ($Values.Count -eq 0) { throw "Median requires at least one value." }
  $ordered = @($Values | Sort-Object)
  $middle = [Math]::Floor($ordered.Count / 2)
  if ($ordered.Count % 2 -eq 1) { return [double]$ordered[$middle] }
  return ([double]$ordered[$middle - 1] + [double]$ordered[$middle]) / 2
}

function Get-FootprintPercentile {
  param(
    [Parameter(Mandatory)][double[]]$Values,
    [Parameter(Mandatory)][ValidateRange(0.000001, 1)][double]$Percentile
  )
  if ($Values.Count -eq 0) { throw "Percentile requires at least one value." }
  $ordered = @($Values | Sort-Object)
  $rank = [Math]::Ceiling($Percentile * $ordered.Count)
  return [double]$ordered[[Math]::Max(0, $rank - 1)]
}

function Get-FootprintCpuPercent {
  param(
    [Parameter(Mandatory)][double]$FirstCpuSeconds,
    [Parameter(Mandatory)][double]$LastCpuSeconds,
    [Parameter(Mandatory)][ValidateRange(0.000001, 3600)][double]$WallSeconds
  )
  if ($LastCpuSeconds -lt $FirstCpuSeconds) {
    throw "CPU counters cannot decrease across a measurement window."
  }
  $delta = $LastCpuSeconds - $FirstCpuSeconds
  return [Math]::Round(($delta / $WallSeconds) * 100, 6)
}

function Get-FootprintCpuMeasurement {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$FirstSample,
    [Parameter(Mandatory)][Collections.IDictionary]$LastSample
  )
  foreach ($sample in @($FirstSample, $LastSample)) {
    foreach ($field in @("coreCpuSeconds", "sampleElapsedSeconds", "coreProcessIdentityKeys", "processes")) {
      if (-not $sample.Contains($field)) {
        throw "CPU sample is missing '$field'."
      }
    }
  }
  $wallSeconds = [double]$LastSample.sampleElapsedSeconds - [double]$FirstSample.sampleElapsedSeconds
  if ($wallSeconds -le 0 -or $wallSeconds -gt 3600) {
    throw "CPU sampling requires a positive measured wall duration of at most one hour."
  }
  $firstKeys = @($FirstSample.coreProcessIdentityKeys)
  $lastKeys = @($LastSample.coreProcessIdentityKeys)
  if ($firstKeys.Count -eq 0 -or $firstKeys.Count -ne $lastKeys.Count -or
      (@(Compare-Object $firstKeys $lastKeys -SyncWindow 0).Count -ne 0)) {
    throw "Core process identity membership changed during CPU sampling."
  }
  $firstByIdentity = @{}
  $lastByIdentity = @{}
  foreach ($record in @($FirstSample.processes | Where-Object coreIncluded)) {
    $firstByIdentity[[string]$record.identityKey] = [double]$record.cpuSeconds
  }
  foreach ($record in @($LastSample.processes | Where-Object coreIncluded)) {
    $lastByIdentity[[string]$record.identityKey] = [double]$record.cpuSeconds
  }
  $cpuDelta = [double]0
  foreach ($key in $firstKeys) {
    if (-not $firstByIdentity.ContainsKey($key) -or -not $lastByIdentity.ContainsKey($key)) {
      throw "CPU sample identity records do not match their canonical membership set."
    }
    $identityDelta = [double]$lastByIdentity[$key] - [double]$firstByIdentity[$key]
    if ($identityDelta -lt 0) { throw "A per-process CPU counter decreased during sampling." }
    $cpuDelta += $identityDelta
  }
  $aggregateDelta = [double]$LastSample.coreCpuSeconds - [double]$FirstSample.coreCpuSeconds
  if ($aggregateDelta -lt 0) { throw "The aggregate core CPU counter decreased during sampling." }
  if ([Math]::Abs($aggregateDelta - $cpuDelta) -gt 0.000001) {
    throw "Aggregate CPU delta does not match the stable per-process identity set."
  }
  return [ordered]@{
    firstCpuSeconds = [double]$FirstSample.coreCpuSeconds
    lastCpuSeconds = [double]$LastSample.coreCpuSeconds
    measuredWallSeconds = [Math]::Round($wallSeconds, 9)
    coreProcessIdentityKeys = $firstKeys
    percentOneCore = [Math]::Round(($cpuDelta / $wallSeconds) * 100, 6)
  }
}

function Assert-FootprintStableCpuSamples {
  param([Parameter(Mandatory)][object[]]$Samples)
  if ($Samples.Count -lt 2) { throw "CPU stability requires at least two samples." }
  $baselineKeys = @($Samples[0].coreProcessIdentityKeys)
  if ($baselineKeys.Count -eq 0) { throw "CPU stability requires at least one core process identity." }
  $previous = $null
  foreach ($sample in $Samples) {
    $keys = @($sample.coreProcessIdentityKeys)
    if ($keys.Count -ne $baselineKeys.Count -or @(Compare-Object $baselineKeys $keys -SyncWindow 0).Count -ne 0) {
      throw "Core process identity membership changed during CPU sampling."
    }
    $current = @{}
    foreach ($record in @($sample.processes | Where-Object coreIncluded)) {
      $current[[string]$record.identityKey] = [double]$record.cpuSeconds
    }
    if ($current.Count -ne $baselineKeys.Count) { throw "CPU sample identity records are incomplete or duplicated." }
    foreach ($key in $baselineKeys) {
      if (-not $current.ContainsKey($key)) { throw "CPU sample identity records do not match their canonical membership set." }
      if ($null -ne $previous -and [double]$current[$key] -lt [double]$previous[$key]) {
        throw "A per-process CPU counter decreased during sampling."
      }
    }
    if ($null -ne $previous -and [double]$sample.coreCpuSeconds -lt [double]$previous.aggregateCpuSeconds) {
      throw "The aggregate core CPU counter decreased during sampling."
    }
    $current["aggregateCpuSeconds"] = [double]$sample.coreCpuSeconds
    $previous = $current
  }
  return $true
}

function Get-FootprintProcessRole {
  param(
    [Parameter(Mandatory)][string]$Name,
    [AllowNull()][string]$CommandLine,
    [Parameter(Mandatory)][bool]$IsRoot
  )
  if ($IsRoot) { return "native-host" }
  $normalized = [IO.Path]::GetFileNameWithoutExtension($Name).ToLowerInvariant()
  $command = if ($null -eq $CommandLine) { "" } else { $CommandLine }
  if ($normalized -eq "msedgewebview2") {
    if ($command -match "(?:^|\s)--type=crashpad-handler(?:\s|$)") { return "crashpad" }
    if ($command -match "(?:^|\s)--type=renderer(?:\s|$)") { return "renderer" }
    if ($command -match "(?:^|\s)--type=gpu-process(?:\s|$)") { return "gpu" }
    if ($command -match "(?:^|\s)--type=utility(?:\s|$)") {
      if ($command -match "network\.mojom\.NetworkService") { return "utility-network" }
      if ($command -match "storage\.mojom\.StorageService") { return "utility-storage" }
      return "utility-other"
    }
    if ($command -match "--embedded-browser-webview=1") { return "browser" }
    return "webview-unknown"
  }
  if (@("cmd", "conhost", "node", "openconsole", "powershell", "pwsh", "windowsterminal", "wsl", "wslhost") -contains $normalized) {
    return "excluded-child"
  }
  return "unknown"
}

function Get-FootprintTreeSample {
  param([Parameter(Mandatory)][int]$RootProcessId)
  $processRows = @(Get-FootprintProcessSnapshot)
  $members = @(Get-FootprintDescendants $RootProcessId $processRows)
  $records = @()
  $identityBytes = [long]0
  $limits = Get-FootprintProcessLimits
  foreach ($member in $members) {
    try {
      $process = Get-Process -Id ([int]$member.ProcessId) -ErrorAction Stop
      $role = Get-FootprintProcessRole $member.Name $member.CommandLine ([int]$member.ProcessId -eq $RootProcessId)
      $coreIncluded = $role -in @("native-host", "browser", "renderer", "gpu", "utility-network", "utility-storage", "utility-other", "crashpad", "webview-unknown")
      $identity = ConvertTo-FootprintProcessIdentity $member
      $identityBytes += $identity.identityBytes
      if ($identityBytes -gt $limits.retainedIdentityBytes) {
        throw "Sampled process identities exceed their $($limits.retainedIdentityBytes)-byte limit."
      }
      $records += [pscustomobject][ordered]@{
        processId = [int]$member.ProcessId
        parentProcessId = [int]$member.ParentProcessId
        name = [string]$member.Name
        executablePath = if ($member.ExecutablePath) { [string]$member.ExecutablePath } else { $null }
        creationDate = $identity.creationDate
        identityKey = "$($identity.processId)`0$($identity.executablePath.ToLowerInvariant())`0$($identity.creationDate)"
        commandLine = if ($member.CommandLine) { [string]$member.CommandLine } else { $null }
        role = $role
        coreIncluded = $coreIncluded
        privateBytes = [long]$process.PrivateMemorySize64
        workingSetBytes = [long]$process.WorkingSet64
        cpuSeconds = if ($null -eq $process.CPU) { 0.0 } else { [double]$process.CPU }
      }
    } catch [Microsoft.PowerShell.Commands.ProcessCommandException] {
      # Membership is refreshed on the next bounded sample.
    }
  }
  $core = @($records | Where-Object coreIncluded)
  $excluded = @($records | Where-Object { $_.role -eq "excluded-child" })
  if ($core.Count -eq 0) {
    $observed = (@($members | Select-Object -First 16 | ForEach-Object { "$($_.ProcessId):$($_.Name)" }) -join ",")
    throw "Footprint sample captured no core candidate process (root $RootProcessId; members $observed)."
  }
  $corePrivateBytes = [long]0
  $coreWorkingSetBytes = [long]0
  $coreCpuSeconds = [double]0
  foreach ($record in $core) {
    $corePrivateBytes += [long]$record.privateBytes
    $coreWorkingSetBytes += [long]$record.workingSetBytes
    $coreCpuSeconds += [double]$record.cpuSeconds
  }
  $excludedPrivateBytes = [long]0
  $excludedWorkingSetBytes = [long]0
  foreach ($record in $excluded) {
    $excludedPrivateBytes += [long]$record.privateBytes
    $excludedWorkingSetBytes += [long]$record.workingSetBytes
  }
  return [ordered]@{
    capturedAtUtc = [DateTime]::UtcNow.ToString("o")
    corePrivateBytes = $corePrivateBytes
    coreWorkingSetBytes = $coreWorkingSetBytes
    coreCpuSeconds = $coreCpuSeconds
    coreProcessIdentityKeys = @($core | ForEach-Object identityKey | Sort-Object)
    excludedPrivateBytes = $excludedPrivateBytes
    excludedWorkingSetBytes = $excludedWorkingSetBytes
    processCount = $records.Count
    unknownProcessCount = @($records | Where-Object { $_.role -in @("unknown", "webview-unknown") }).Count
    processes = $records
  }
}

function Wait-FootprintWindow {
  param(
    [Parameter(Mandatory)][int]$ProcessId,
    [Parameter(Mandatory)][ValidateRange(1, 120)][int]$TimeoutSeconds,
    [string]$ExpectedTitle = "Sage"
  )
  $timer = [Diagnostics.Stopwatch]::StartNew()
  while ($timer.Elapsed.TotalSeconds -lt $TimeoutSeconds) {
    $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($null -eq $process) { throw "Target process exited before its window became available." }
    if ($process.MainWindowHandle -ne [IntPtr]::Zero -and $process.MainWindowTitle -eq $ExpectedTitle) {
      return [ordered]@{ handle = $process.MainWindowHandle; elapsedMs = [Math]::Round($timer.Elapsed.TotalMilliseconds, 3); title = $process.MainWindowTitle }
    }
    Start-Sleep -Milliseconds 20
  }
  throw "The exact '$ExpectedTitle' window did not appear within $TimeoutSeconds seconds."
}

Export-ModuleMember -Function @(
  "Assert-FootprintStableCpuSamples",
  "Get-FootprintCpuMeasurement",
  "Get-FootprintCpuPercent",
  "Get-FootprintMedian",
  "Get-FootprintPercentile",
  "Get-FootprintProcessRole",
  "Get-FootprintTreeSample",
  "Wait-FootprintWindow"
)
