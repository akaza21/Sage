Set-StrictMode -Version Latest

$script:MaxProcessSnapshotRows = 4096
$script:MaxRetainedProcessRecords = 128
$script:MaxRetainedIdentityBytes = 64KB
$script:PostKillWaitSeconds = 5

function Get-FootprintProcessLimits {
  return [ordered]@{
    systemSnapshotRows = $script:MaxProcessSnapshotRows
    retainedProcessRecords = $script:MaxRetainedProcessRecords
    retainedIdentityBytes = $script:MaxRetainedIdentityBytes
    postKillWaitSeconds = $script:PostKillWaitSeconds
  }
}

function Get-FootprintProcessSnapshot {
  $rows = @(Get-CimInstance Win32_Process)
  if ($rows.Count -gt $script:MaxProcessSnapshotRows) {
    throw "Process snapshot exceeds its $($script:MaxProcessSnapshotRows)-row limit."
  }
  return $rows
}

function Get-FootprintDescendants {
  param(
    [Parameter(Mandatory)][int]$RootProcessId,
    [AllowNull()][object[]]$ProcessRows,
    [ValidateRange(1, 4096)][int]$MaxProcessRows = $script:MaxProcessSnapshotRows,
    [ValidateRange(1, 512)][int]$MaxDescendants = $script:MaxRetainedProcessRecords
  )
  if ($null -eq $ProcessRows) { $ProcessRows = @(Get-FootprintProcessSnapshot) }
  if ($ProcessRows.Count -gt $MaxProcessRows) {
    throw "Process snapshot exceeds its $MaxProcessRows-row limit."
  }
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
  $result = @($ProcessRows | Where-Object { $included.Contains([int]$_.ProcessId) } | Sort-Object ProcessId)
  if ($result.Count -gt $MaxDescendants) {
    throw "Candidate tree exceeds its $MaxDescendants-process limit."
  }
  return $result
}

function ConvertTo-FootprintCreationIdentity {
  param([Parameter(Mandatory)]$CreationDate)
  if ($CreationDate -is [DateTime]) {
    return $CreationDate.ToUniversalTime().ToString(
      "yyyy-MM-ddTHH:mm:ss.fff'Z'",
      [Globalization.CultureInfo]::InvariantCulture
    )
  }
  return [string]$CreationDate
}

function ConvertTo-FootprintProcessIdentity {
  param([Parameter(Mandatory)]$ProcessRow)
  if (-not $ProcessRow.ExecutablePath -or -not $ProcessRow.CreationDate) {
    throw "A remembered process lacks executable or creation identity."
  }
  $path = [IO.Path]::GetFullPath([string]$ProcessRow.ExecutablePath)
  $creation = ConvertTo-FootprintCreationIdentity $ProcessRow.CreationDate
  return [ordered]@{
    processId = [int]$ProcessRow.ProcessId
    executablePath = $path
    creationDate = $creation
    identityBytes = [Text.Encoding]::UTF8.GetByteCount("$path`0$creation")
  }
}

function New-FootprintIdentityStore {
  return [Collections.Generic.Dictionary[string, object]]::new([StringComparer]::Ordinal)
}

function Add-FootprintRememberedIdentity {
  param(
    [Parameter(Mandatory)][Collections.Generic.Dictionary[string, object]]$Store,
    [Parameter(Mandatory)]$ProcessRow
  )
  $identity = ConvertTo-FootprintProcessIdentity $ProcessRow
  $key = "$($identity.processId)`0$($identity.executablePath.ToLowerInvariant())`0$($identity.creationDate)"
  if ($Store.ContainsKey($key)) { return $identity }
  if ($Store.Count -ge $script:MaxRetainedProcessRecords) {
    throw "Remembered process identities exceed their $($script:MaxRetainedProcessRecords)-record limit."
  }
  $retainedBytes = [long]0
  foreach ($remembered in $Store.Values) { $retainedBytes += [long]$remembered.identityBytes }
  if ($retainedBytes + $identity.identityBytes -gt $script:MaxRetainedIdentityBytes) {
    throw "Remembered process identities exceed their $($script:MaxRetainedIdentityBytes)-byte limit."
  }
  $Store.Add($key, $identity)
  return $identity
}

function Test-FootprintProcessIdentity {
  param(
    [Parameter(Mandatory)]$ProcessRow,
    [Parameter(Mandatory)][string]$ExpectedExecutablePath,
    [Parameter(Mandatory)][string]$ExpectedCreationDate
  )
  if (-not $ProcessRow.ExecutablePath -or -not $ProcessRow.CreationDate) { return $false }
  return [string]::Equals(
    [IO.Path]::GetFullPath([string]$ProcessRow.ExecutablePath),
    [IO.Path]::GetFullPath($ExpectedExecutablePath),
    [StringComparison]::OrdinalIgnoreCase
  ) -and (ConvertTo-FootprintCreationIdentity $ProcessRow.CreationDate) -eq $ExpectedCreationDate
}

function Get-FootprintRootIdentity {
  param(
    [Parameter(Mandatory)][int]$RootProcessId,
    [Parameter(Mandatory)][string]$ExpectedExecutablePath,
    [AllowNull()][string]$ExpectedCreationDate,
    [AllowNull()][object[]]$ProcessRows
  )
  if ($null -eq $ProcessRows) { $ProcessRows = @(Get-FootprintProcessSnapshot) }
  if ($ProcessRows.Count -gt $script:MaxProcessSnapshotRows) {
    throw "Process snapshot exceeds its $($script:MaxProcessSnapshotRows)-row limit."
  }
  $root = @($ProcessRows | Where-Object { [int]$_.ProcessId -eq $RootProcessId }) | Select-Object -First 1
  if ($null -eq $root) { throw "The launched root lacks exact executable or creation identity." }
  $identity = ConvertTo-FootprintProcessIdentity $root
  if (-not [string]::Equals($identity.executablePath, [IO.Path]::GetFullPath($ExpectedExecutablePath), [StringComparison]::OrdinalIgnoreCase)) {
    throw "The launched root executable identity does not match the target."
  }
  if ($ExpectedCreationDate -and $identity.creationDate -ne $ExpectedCreationDate) {
    throw "The launched root creation identity does not match the target."
  }
  return $identity
}

function Get-FootprintLaunchIdentity {
  param([Parameter(Mandatory)]$Process, [Parameter(Mandatory)][string]$ExpectedExecutablePath)
  $path = $Process.Path
  if (-not $path -or -not [string]::Equals([IO.Path]::GetFullPath($path), [IO.Path]::GetFullPath($ExpectedExecutablePath), [StringComparison]::OrdinalIgnoreCase)) {
    throw "The launched process object does not identify the exact executable."
  }
  return ConvertTo-FootprintProcessIdentity ([pscustomobject]@{
    ProcessId = $Process.Id
    ExecutablePath = $ExpectedExecutablePath
    CreationDate = $Process.StartTime
  })
}

function Stop-FootprintLaunchedProcessObject {
  param(
    [Parameter(Mandatory)]$Process,
    [Parameter(Mandatory)][string]$ExpectedExecutablePath,
    [AllowNull()][Collections.Generic.Dictionary[string, object]]$KnownProcessIdentities,
    [AllowNull()][object[]]$ProcessRows,
    [ValidateRange(0, 5000)][int]$GraceMilliseconds = 1000,
    [ValidateRange(100, 10000)][int]$ForceWaitMilliseconds = 5000
  )
  if ($null -eq $KnownProcessIdentities) { $KnownProcessIdentities = New-FootprintIdentityStore }
  $boundaryError = $null
  $initiallyExited = $false
  try {
    $Process.Refresh()
    $initiallyExited = $Process.HasExited
  } catch {
    $boundaryError = $_.Exception.Message
  }
  if ($initiallyExited) {
    return [ordered]@{ forced = $false; exited = $true; treeBoundaryVerified = $false }
  }
  $expectedCreationDate = $null
  try {
    $expectedCreationDate = ConvertTo-FootprintCreationIdentity $Process.StartTime
  } catch {
    $boundaryError = $_.Exception.Message
  }
  $rootIdentity = $null
  $snapshotMatched = $false
  $identityConflict = $false
  try {
    if ($null -eq $ProcessRows) { $ProcessRows = @(Get-FootprintProcessSnapshot) }
    if ($ProcessRows.Count -gt $script:MaxProcessSnapshotRows) {
      throw "Process snapshot exceeds its $($script:MaxProcessSnapshotRows)-row limit."
    }
    $rootRows = @($ProcessRows | Where-Object { [int]$_.ProcessId -eq [int]$Process.Id })
    if ($expectedCreationDate -and $rootRows.Count -eq 1 -and
        (Test-FootprintProcessIdentity $rootRows[0] $ExpectedExecutablePath $expectedCreationDate)) {
      $rootIdentity = ConvertTo-FootprintProcessIdentity $rootRows[0]
      $snapshotMatched = $true
      foreach ($row in @(Get-FootprintDescendants ([int]$rootIdentity.processId) $ProcessRows)) {
        [void](Add-FootprintRememberedIdentity $KnownProcessIdentities $row)
      }
    } elseif ($expectedCreationDate -and $rootRows.Count -gt 0) {
      $identityConflict = $true
    }
  } catch {
    $boundaryError = $_.Exception.Message
  }
  if ($identityConflict) {
    throw "Current CIM root reuses the launched PID without matching its exact path and creation identity."
  }
  try {
    if ($Process.MainWindowHandle -ne [IntPtr]::Zero) {
      [void]$Process.CloseMainWindow()
    }
  } catch {
    # Failure to request graceful close must not prevent exact handle-bound kill.
  }
  $forced = $false
  $treeKillCompleted = $false
  $gracefulExit = $false
  if ($GraceMilliseconds -gt 0) {
    try { $gracefulExit = $Process.WaitForExit($GraceMilliseconds) } catch { $boundaryError = $_.Exception.Message }
  }
  if (-not $gracefulExit) {
    $stillRunning = $true
    try {
      $Process.Refresh()
      $stillRunning = -not $Process.HasExited
    } catch {
      $boundaryError = $_.Exception.Message
    }
    if ($stillRunning) {
      $forced = $true
      try {
        $Process.Kill($true)
        $treeKillCompleted = $true
      } catch {
        $Process.Kill()
      }
    }
  }
  $exited = $false
  try { $exited = $Process.WaitForExit($ForceWaitMilliseconds) } catch { $boundaryError = $_.Exception.Message }
  try {
    $Process.Refresh()
    $exited = $exited -or $Process.HasExited
  } catch {
    $exited = $false
    $boundaryError = $_.Exception.Message
  }
  if (-not $exited) {
    throw "Exact launched process-object cleanup did not complete within its bounded wait."
  }
  if ($snapshotMatched) {
    $treeTeardown = Stop-FootprintTree $rootIdentity $KnownProcessIdentities 1
    if ($treeTeardown.identityLost -or $treeTeardown.remaining.Count -ne 0) {
      throw "Exact launched process-object cleanup could not verify its remembered process tree."
    }
  } elseif (-not $treeKillCompleted) {
    $detail = if ($boundaryError) { " $boundaryError" } else { "" }
    throw "Exact launched process-object cleanup could not establish a process-tree boundary.$detail"
  }
  return [ordered]@{
    forced = $forced
    exited = $true
    treeBoundaryVerified = $snapshotMatched -or $treeKillCompleted
  }
}

function Get-FootprintTeardownPlan {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$RootIdentity,
    [Parameter(Mandatory)][Collections.Generic.Dictionary[string, object]]$RememberedIdentities,
    [AllowNull()][object[]]$ProcessRows
  )
  if ($null -eq $ProcessRows) { $ProcessRows = @(Get-FootprintProcessSnapshot) }
  if ($ProcessRows.Count -gt $script:MaxProcessSnapshotRows) {
    throw "Process snapshot exceeds its $($script:MaxProcessSnapshotRows)-row limit."
  }
  $rootRows = @($ProcessRows | Where-Object { [int]$_.ProcessId -eq [int]$RootIdentity.processId })
  $rootState = if ($rootRows.Count -eq 0) { "absent" } elseif (
    @($rootRows | Where-Object { Test-FootprintProcessIdentity $_ $RootIdentity.executablePath $RootIdentity.creationDate }).Count -eq 1
  ) { "matching" } else { "reused" }
  $candidates = @()
  $reusedProcessIds = @()
  foreach ($row in $ProcessRows) {
    $samePid = @($RememberedIdentities.Values | Where-Object { [int]$_.processId -eq [int]$row.ProcessId })
    if ($samePid.Count -eq 0) { continue }
    $match = @($samePid | Where-Object { Test-FootprintProcessIdentity $row $_.executablePath $_.creationDate }) | Select-Object -First 1
    if ($null -ne $match) { $candidates += $row } else { $reusedProcessIds += [int]$row.ProcessId }
  }
  return [ordered]@{
    rootState = $rootState
    identityLost = $rootState -eq "reused"
    candidates = @($candidates | Sort-Object ProcessId)
    reusedProcessIds = @($reusedProcessIds | Sort-Object -Unique)
  }
}

function Get-FootprintForceKillCandidates {
  param([Parameter(Mandatory)][Collections.IDictionary]$RootIdentity, [AllowNull()][object[]]$ProcessRows)
  if ($null -eq $ProcessRows) { $ProcessRows = @(Get-FootprintProcessSnapshot) }
  $root = @($ProcessRows | Where-Object { [int]$_.ProcessId -eq [int]$RootIdentity.processId }) | Select-Object -First 1
  if ($null -eq $root -or -not (Test-FootprintProcessIdentity $root $RootIdentity.executablePath $RootIdentity.creationDate)) { return @() }
  return @(Get-FootprintDescendants ([int]$RootIdentity.processId) $ProcessRows)
}

function Stop-FootprintTree {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$RootIdentity,
    [Parameter(Mandatory)][Collections.Generic.Dictionary[string, object]]$KnownProcessIdentities,
    [ValidateRange(1, 30)][int]$GraceSeconds = 1
  )
  $rootPid = [int]$RootIdentity.processId
  [void](Add-FootprintRememberedIdentity $KnownProcessIdentities ([pscustomobject]@{ ProcessId = $rootPid; ExecutablePath = $RootIdentity.executablePath; CreationDate = $RootIdentity.creationDate }))
  $initialRows = @(Get-FootprintProcessSnapshot)
  $plan = Get-FootprintTeardownPlan $RootIdentity $KnownProcessIdentities $initialRows
  $identityLost = [bool]$plan.identityLost
  if ($plan.rootState -eq "matching") {
    foreach ($row in @(Get-FootprintDescendants $rootPid $initialRows)) { [void](Add-FootprintRememberedIdentity $KnownProcessIdentities $row) }
    $plan = Get-FootprintTeardownPlan $RootIdentity $KnownProcessIdentities $initialRows
    $root = Get-Process -Id $rootPid -ErrorAction SilentlyContinue
    if ($null -ne $root -and $root.MainWindowHandle -ne [IntPtr]::Zero) { [void]$root.CloseMainWindow() }
  }
  $deadline = [DateTime]::UtcNow.AddSeconds($GraceSeconds)
  do {
    $plan = Get-FootprintTeardownPlan $RootIdentity $KnownProcessIdentities @(Get-FootprintProcessSnapshot)
    $identityLost = $identityLost -or $plan.identityLost
    if ($plan.candidates.Count -eq 0) { return [ordered]@{ forced = $false; identityLost = $identityLost; candidates = @(); remaining = @(); reusedProcessIds = $plan.reusedProcessIds } }
    Start-Sleep -Milliseconds 100
  } while ([DateTime]::UtcNow -lt $deadline)
  $plan = Get-FootprintTeardownPlan $RootIdentity $KnownProcessIdentities @(Get-FootprintProcessSnapshot)
  $identityLost = $identityLost -or $plan.identityLost
  $candidates = @($plan.candidates | Sort-Object @{ Expression = { [int]$_.ProcessId -eq $rootPid } }, @{ Expression = { [int]$_.ProcessId }; Descending = $true })
  $killed = @()
  foreach ($candidate in $candidates) {
    $currentPlan = Get-FootprintTeardownPlan $RootIdentity $KnownProcessIdentities @(Get-FootprintProcessSnapshot)
    $identityLost = $identityLost -or $currentPlan.identityLost
    $current = @($currentPlan.candidates | Where-Object { [int]$_.ProcessId -eq [int]$candidate.ProcessId }) | Select-Object -First 1
    if ($null -eq $current -or -not (Test-FootprintProcessIdentity $current $candidate.ExecutablePath (ConvertTo-FootprintCreationIdentity $candidate.CreationDate))) { continue }
    Stop-Process -Id ([int]$current.ProcessId) -Force -ErrorAction SilentlyContinue
    $killed += ConvertTo-FootprintProcessIdentity $current
  }
  $postKillDeadline = [DateTime]::UtcNow.AddSeconds($script:PostKillWaitSeconds)
  do {
    $remainingPlan = Get-FootprintTeardownPlan $RootIdentity $KnownProcessIdentities @(Get-FootprintProcessSnapshot)
    $identityLost = $identityLost -or $remainingPlan.identityLost
    if ($remainingPlan.candidates.Count -eq 0) { break }
    Start-Sleep -Milliseconds 100
  } while ([DateTime]::UtcNow -lt $postKillDeadline)
  return [ordered]@{
    forced = $killed.Count -gt 0
    identityLost = $identityLost
    candidates = @($candidates | ForEach-Object ProcessId)
    remaining = @($remainingPlan.candidates | ForEach-Object ProcessId)
    reusedProcessIds = $remainingPlan.reusedProcessIds
  }
}

Export-ModuleMember -Function @(
  "Add-FootprintRememberedIdentity",
  "ConvertTo-FootprintProcessIdentity",
  "Get-FootprintDescendants",
  "Get-FootprintForceKillCandidates",
  "Get-FootprintLaunchIdentity",
  "Get-FootprintProcessLimits",
  "Get-FootprintProcessSnapshot",
  "Get-FootprintRootIdentity",
  "Get-FootprintTeardownPlan",
  "New-FootprintIdentityStore",
  "Stop-FootprintLaunchedProcessObject",
  "Stop-FootprintTree"
)
