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
  $delta = [Math]::Max(0.0, $LastCpuSeconds - $FirstCpuSeconds)
  return [Math]::Round(($delta / $WallSeconds) * 100, 6)
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
  "Get-FootprintCpuPercent",
  "Get-FootprintMedian",
  "Get-FootprintPercentile",
  "Get-FootprintProcessRole",
  "Get-FootprintTreeSample",
  "Wait-FootprintWindow"
)
