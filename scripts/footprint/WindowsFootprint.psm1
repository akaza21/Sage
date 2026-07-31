Set-StrictMode -Version Latest

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

function Get-FootprintDescendants {
  param(
    [Parameter(Mandatory)][int]$RootProcessId,
    [AllowNull()][object[]]$ProcessRows
  )
  if ($null -eq $ProcessRows) { $ProcessRows = @(Get-CimInstance Win32_Process) }
  $included = [Collections.Generic.HashSet[int]]::new()
  [void]$included.Add($RootProcessId)
  do {
    $changed = $false
    foreach ($row in $ProcessRows) {
      $pid = [int]$row.ProcessId
      if (-not $included.Contains($pid) -and $included.Contains([int]$row.ParentProcessId)) {
        [void]$included.Add($pid)
        $changed = $true
      }
    }
  } while ($changed)
  return @($ProcessRows | Where-Object { $included.Contains([int]$_.ProcessId) } | Sort-Object ProcessId)
}

function ConvertTo-FootprintCreationIdentity {
  param([Parameter(Mandatory)]$CreationDate)
  if ($CreationDate -is [DateTime]) { return $CreationDate.ToUniversalTime().ToString("o") }
  return [string]$CreationDate
}

function Test-FootprintProcessIdentity {
  param(
    [Parameter(Mandatory)]$ProcessRow,
    [Parameter(Mandatory)][string]$ExpectedExecutablePath,
    [Parameter(Mandatory)][string]$ExpectedCreationDate
  )
  if (-not $ProcessRow.ExecutablePath -or -not $ProcessRow.CreationDate) { return $false }
  $pathMatches = [string]::Equals(
    [IO.Path]::GetFullPath([string]$ProcessRow.ExecutablePath),
    [IO.Path]::GetFullPath($ExpectedExecutablePath),
    [StringComparison]::OrdinalIgnoreCase
  )
  return $pathMatches -and (
    (ConvertTo-FootprintCreationIdentity $ProcessRow.CreationDate) -eq $ExpectedCreationDate
  )
}

function Get-FootprintRootIdentity {
  param(
    [Parameter(Mandatory)][int]$RootProcessId,
    [Parameter(Mandatory)][string]$ExpectedExecutablePath,
    [AllowNull()][object[]]$ProcessRows
  )
  if ($null -eq $ProcessRows) { $ProcessRows = @(Get-CimInstance Win32_Process) }
  $root = @($ProcessRows | Where-Object { [int]$_.ProcessId -eq $RootProcessId }) | Select-Object -First 1
  if ($null -eq $root -or -not $root.ExecutablePath -or -not $root.CreationDate) {
    throw "The launched root lacks exact executable or creation identity."
  }
  $expected = [IO.Path]::GetFullPath($ExpectedExecutablePath)
  if (-not [string]::Equals([IO.Path]::GetFullPath([string]$root.ExecutablePath), $expected, [StringComparison]::OrdinalIgnoreCase)) {
    throw "The launched root executable identity does not match the target."
  }
  return [ordered]@{
    processId = $RootProcessId
    executablePath = $expected
    creationDate = ConvertTo-FootprintCreationIdentity $root.CreationDate
  }
}

function Get-FootprintForceKillCandidates {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$RootIdentity,
    [AllowNull()][object[]]$ProcessRows
  )
  if ($null -eq $ProcessRows) { $ProcessRows = @(Get-CimInstance Win32_Process) }
  $root = @($ProcessRows | Where-Object { [int]$_.ProcessId -eq [int]$RootIdentity.processId }) | Select-Object -First 1
  if ($null -eq $root -or -not (Test-FootprintProcessIdentity $root $RootIdentity.executablePath $RootIdentity.creationDate)) {
    return @()
  }
  return @(Get-FootprintDescendants ([int]$RootIdentity.processId) $ProcessRows)
}

function Get-FootprintTreeSample {
  param([Parameter(Mandatory)][int]$RootProcessId)
  $members = @(Get-FootprintDescendants $RootProcessId @(Get-CimInstance Win32_Process))
  $records = @()
  foreach ($member in $members) {
    try {
      $process = Get-Process -Id ([int]$member.ProcessId) -ErrorAction Stop
      $role = Get-FootprintProcessRole $member.Name $member.CommandLine ([int]$member.ProcessId -eq $RootProcessId)
      $coreIncluded = $role -in @("native-host", "browser", "renderer", "gpu", "utility-network", "utility-storage", "utility-other", "crashpad", "webview-unknown")
      $records += [ordered]@{
        processId = [int]$member.ProcessId
        parentProcessId = [int]$member.ParentProcessId
        name = [string]$member.Name
        executablePath = if ($member.ExecutablePath) { [string]$member.ExecutablePath } else { $null }
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
  return [ordered]@{
    capturedAtUtc = [DateTime]::UtcNow.ToString("o")
    corePrivateBytes = [long](($core | Measure-Object privateBytes -Sum).Sum)
    coreWorkingSetBytes = [long](($core | Measure-Object workingSetBytes -Sum).Sum)
    coreCpuSeconds = [double](($core | Measure-Object cpuSeconds -Sum).Sum)
    excludedPrivateBytes = [long](($excluded | Measure-Object privateBytes -Sum).Sum)
    excludedWorkingSetBytes = [long](($excluded | Measure-Object workingSetBytes -Sum).Sum)
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

function Stop-FootprintTree {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$RootIdentity,
    [Parameter(Mandatory)][Collections.Generic.HashSet[int]]$KnownProcessIds,
    [ValidateRange(1, 30)][int]$GraceSeconds = 1
  )
  $rootPid = [int]$RootIdentity.processId
  $initialRows = @(Get-CimInstance Win32_Process)
  $initialRoot = @($initialRows | Where-Object { [int]$_.ProcessId -eq $rootPid }) | Select-Object -First 1
  if ($null -eq $initialRoot) {
    return [ordered]@{ forced = $false; identityLost = $false; candidates = @(); remaining = @() }
  }
  if (-not (Test-FootprintProcessIdentity $initialRoot $RootIdentity.executablePath $RootIdentity.creationDate)) {
    return [ordered]@{ forced = $false; identityLost = $true; candidates = @(); remaining = @() }
  }
  $initial = @(Get-FootprintForceKillCandidates $RootIdentity $initialRows)
  $root = Get-Process -Id $rootPid -ErrorAction SilentlyContinue
  if ($null -ne $root -and $root.MainWindowHandle -ne [IntPtr]::Zero) { [void]$root.CloseMainWindow() }
  $deadline = [DateTime]::UtcNow.AddSeconds($GraceSeconds)
  do {
    $members = @(Get-FootprintForceKillCandidates $RootIdentity @(Get-CimInstance Win32_Process))
    foreach ($row in $members) { [void]$KnownProcessIds.Add([int]$row.ProcessId) }
    if ($members.Count -eq 0) { return [ordered]@{ forced = $false; identityLost = $false; candidates = @(); remaining = @() } }
    Start-Sleep -Milliseconds 100
  } while ([DateTime]::UtcNow -lt $deadline)
  $candidates = @(Get-FootprintForceKillCandidates $RootIdentity @(Get-CimInstance Win32_Process) | Sort-Object @{ Expression = { [int]$_.ProcessId -eq $rootPid } }, @{ Expression = { [int]$_.ProcessId }; Descending = $true })
  $killed = @()
  foreach ($candidate in $candidates) {
    $current = @(Get-FootprintForceKillCandidates $RootIdentity @(Get-CimInstance Win32_Process) | Where-Object { [int]$_.ProcessId -eq [int]$candidate.ProcessId }) | Select-Object -First 1
    if ($null -eq $current -or -not $candidate.ExecutablePath -or -not $candidate.CreationDate) { continue }
    if (-not (Test-FootprintProcessIdentity $current $candidate.ExecutablePath (ConvertTo-FootprintCreationIdentity $candidate.CreationDate))) { continue }
    Stop-Process -Id ([int]$current.ProcessId) -Force -ErrorAction SilentlyContinue
    $killed += [ordered]@{ processId = [int]$current.ProcessId; executablePath = [string]$current.ExecutablePath; creationDate = ConvertTo-FootprintCreationIdentity $current.CreationDate }
  }
  Start-Sleep -Milliseconds 250
  $finalRows = @(Get-CimInstance Win32_Process)
  $remaining = @($killed | Where-Object { $identity = $_; $current = @($finalRows | Where-Object { [int]$_.ProcessId -eq [int]$identity.processId }) | Select-Object -First 1; $null -ne $current -and (Test-FootprintProcessIdentity $current $identity.executablePath $identity.creationDate) } | ForEach-Object processId)
  return [ordered]@{ forced = $killed.Count -gt 0; identityLost = $false; candidates = @($candidates | ForEach-Object ProcessId); remaining = $remaining }
}

Export-ModuleMember -Function @(
  "Get-FootprintCpuPercent",
  "Get-FootprintDescendants",
  "Get-FootprintForceKillCandidates",
  "Get-FootprintMedian",
  "Get-FootprintPercentile",
  "Get-FootprintProcessRole",
  "Get-FootprintRootIdentity",
  "Get-FootprintTreeSample",
  "Stop-FootprintTree",
  "Wait-FootprintWindow"
)
