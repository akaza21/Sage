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
  $rows = @(Get-CimInstance Win32_Process -Property @(
    "ProcessId", "ParentProcessId", "Name", "ExecutablePath", "CreationDate", "CommandLine",
    "PrivatePageCount", "WorkingSetSize", "KernelModeTime", "UserModeTime"
  ) -ErrorAction Stop)
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
    [ValidateRange(1, 512)][int]$MaxDescendants = $script:MaxRetainedProcessRecords,
    [AllowNull()]$RootCreationDate
  )
  if ($null -eq $ProcessRows) { $ProcessRows = @(Get-FootprintProcessSnapshot) }
  if ($ProcessRows.Count -gt $MaxProcessRows) {
    throw "Process snapshot exceeds its $MaxProcessRows-row limit."
  }
  # Windows keeps a process's parent PID after the parent exits, and PIDs are
  # reused, so a row is adopted only when its parent's creation time is known
  # and the row started no earlier than that parent. A row whose own creation
  # time is unknown is still adopted, so its incomplete identity fails closed
  # downstream, but nothing is adopted through it. The root's creation time
  # comes from -RootCreationDate when given, which also rejects a reused root
  # PID, and otherwise from its snapshot row.
  $creationById = @{}
  foreach ($row in $ProcessRows) {
    $creationById[[int]$row.ProcessId] = Get-FootprintCreationTime $row
  }
  $rootCreated = $creationById[$RootProcessId]
  if ($null -ne $RootCreationDate) {
    $expectedRootCreated = ConvertTo-FootprintCreationTime $RootCreationDate
    if ($null -eq $expectedRootCreated) { throw "The root creation time is not a valid timestamp." }
    $expectedRootIdentity = ConvertTo-FootprintCreationIdentity $expectedRootCreated
    if ($null -ne $rootCreated -and (ConvertTo-FootprintCreationIdentity $rootCreated) -ne $expectedRootIdentity) {
      return @()
    }
    $creationById[$RootProcessId] = $expectedRootCreated
  }
  $included = [Collections.Generic.HashSet[int]]::new()
  [void]$included.Add($RootProcessId)
  do {
    $changed = $false
    foreach ($row in $ProcessRows) {
      $processId = [int]$row.ProcessId
      $parentId = [int]$row.ParentProcessId
      if ($included.Contains($processId) -or -not $included.Contains($parentId)) { continue }
      $created = $creationById[$processId]
      $parentCreated = $creationById[$parentId]
      if ($null -eq $parentCreated) { continue }
      if ($null -ne $created -and $created -lt $parentCreated) { continue }
      [void]$included.Add($processId)
      $changed = $true
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

function ConvertTo-FootprintCreationTime {
  param([AllowNull()]$CreationDate)
  if ($CreationDate -is [DateTime]) { return $CreationDate.ToUniversalTime() }
  $parsed = [DateTime]::MinValue
  if ($CreationDate -is [string] -and [DateTime]::TryParse(
      $CreationDate,
      [Globalization.CultureInfo]::InvariantCulture,
      [Globalization.DateTimeStyles]::AdjustToUniversal -bor [Globalization.DateTimeStyles]::AssumeUniversal,
      [ref]$parsed
    )) {
    return $parsed
  }
  return $null
}

function Get-FootprintCreationTime {
  param([Parameter(Mandatory)]$ProcessRow)
  $property = $ProcessRow.PSObject.Properties["CreationDate"]
  if ($null -eq $property) { return $null }
  return ConvertTo-FootprintCreationTime $property.Value
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

function Add-FootprintRememberedIdentities {
  param(
    [Parameter(Mandatory)][Collections.Generic.Dictionary[string, object]]$Store,
    [AllowEmptyCollection()][object[]]$ProcessRows
  )
  # Stage every row first so a row that lacks identity or exceeds a limit
  # leaves the store unchanged instead of partly filled.
  $staged = [Collections.Generic.Dictionary[string, object]]::new($Store, [StringComparer]::Ordinal)
  foreach ($row in $ProcessRows) { [void](Add-FootprintRememberedIdentity $staged $row) }
  foreach ($entry in $staged.GetEnumerator()) {
    if (-not $Store.ContainsKey($entry.Key)) { $Store.Add($entry.Key, $entry.Value) }
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

function Get-FootprintNativeProcessIdentity {
  param(
    [Parameter(Mandatory)]$Process,
    [Parameter(Mandatory)][string]$ExpectedExecutablePath,
    [Parameter(Mandatory)][ValidatePattern("^[0-9a-f]{64}$")][string]$ExecutableIdentitySha256
  )
  $Process.Refresh()
  if ($Process.HasExited) { throw "The launched process exited before its native identity was captured." }
  $actualPath = [IO.Path]::GetFullPath([string]$Process.Path)
  $expectedPath = [IO.Path]::GetFullPath($ExpectedExecutablePath)
  if (-not [string]::Equals($actualPath, $expectedPath, [StringComparison]::OrdinalIgnoreCase)) {
    throw "The launched process object does not identify the expected executable."
  }
  $started = $Process.StartTime.ToUniversalTime()
  $filetime = $started.ToFileTimeUtc().ToString([Globalization.CultureInfo]::InvariantCulture)
  $canonical = '{"creationTimeFiletime100ns":"' + $filetime + '","executableIdentitySha256":"' +
    $ExecutableIdentitySha256 + '","pid":' + ([int]$Process.Id).ToString([Globalization.CultureInfo]::InvariantCulture) + '}'
  $hasher = [Security.Cryptography.SHA256]::Create()
  try {
    $digest = ([BitConverter]::ToString($hasher.ComputeHash([Text.Encoding]::UTF8.GetBytes($canonical)))).Replace("-", "").ToLowerInvariant()
  } finally {
    $hasher.Dispose()
  }
  return [ordered]@{
    pid = [int]$Process.Id
    creationTimeUtc = $started.ToString("yyyy-MM-ddTHH:mm:ss.fff'Z'", [Globalization.CultureInfo]::InvariantCulture)
    creationTimeFiletime100ns = $filetime
    processIdentitySha256 = $digest
  }
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
  $expectedCreationDate = $null
  try {
    $expectedCreationDate = ConvertTo-FootprintCreationIdentity $Process.StartTime
  } catch {
    $boundaryError = $_.Exception.Message
  }
  $rootIdentity = $null
  $snapshotMatched = $false
  $identityConflict = $false
  $processObjectPathMatched = $false
  try {
    $processObjectPathMatched = [string]::Equals(
      [IO.Path]::GetFullPath([string]$Process.Path),
      [IO.Path]::GetFullPath($ExpectedExecutablePath),
      [StringComparison]::OrdinalIgnoreCase
    )
  } catch {
    $boundaryError = $_.Exception.Message
  }
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
    } elseif ($expectedCreationDate -and $rootRows.Count -gt 0) {
      $identityConflict = $true
    } elseif ($initiallyExited -and $expectedCreationDate) {
      $rememberedRoot = @($KnownProcessIdentities.Values | Where-Object {
          [int]$_.processId -eq [int]$Process.Id -and
          (Test-FootprintProcessIdentity `
            ([pscustomobject]@{
              ProcessId = $_.processId
              ExecutablePath = $_.executablePath
              CreationDate = $_.creationDate
            }) `
            $ExpectedExecutablePath `
            $expectedCreationDate)
        }) | Select-Object -First 1
      if ($null -ne $rememberedRoot) {
        $rootIdentity = $rememberedRoot
        $snapshotMatched = $true
      } elseif ($processObjectPathMatched) {
        $rootIdentity = ConvertTo-FootprintProcessIdentity ([pscustomobject]@{
            ProcessId = $Process.Id
            ExecutablePath = $ExpectedExecutablePath
            CreationDate = $expectedCreationDate
          })
        $snapshotMatched = $true
      }
    }
    if ($null -ne $rootIdentity) {
      Add-FootprintRememberedIdentities $KnownProcessIdentities @(
        Get-FootprintDescendants ([int]$rootIdentity.processId) $ProcessRows -RootCreationDate $rootIdentity.creationDate
      )
    }
  } catch {
    # A tree whose members cannot all be identified grants no snapshot authority.
    $snapshotMatched = $false
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

function Get-FootprintTrackedProcessState {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$Identity,
    [AllowEmptyCollection()][object[]]$ProcessRows
  )
  $rows = @($ProcessRows | Where-Object { [int]$_.ProcessId -eq [int]$Identity.processId })
  if ($rows.Count -eq 0) { return "absent" }
  if ($rows.Count -ne 1) { return "lost" }
  $created = Get-FootprintCreationTime $rows[0]
  $expectedCreated = ConvertTo-FootprintCreationTime $Identity.creationDate
  if ($null -eq $created -or $null -eq $expectedCreated) { return "lost" }
  # Windows cannot reuse a PID until the original process is gone. A known
  # different creation time proves exit, even if the new executable is hidden.
  if ((ConvertTo-FootprintCreationIdentity $created) -ne (ConvertTo-FootprintCreationIdentity $expectedCreated)) { return "reused" }
  if (Test-FootprintProcessIdentity $rows[0] $Identity.executablePath $Identity.creationDate) { return "matching" }
  return "lost"
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
  $rootState = Get-FootprintTrackedProcessState $RootIdentity $ProcessRows
  $identityLost = $rootState -eq "lost"
  $candidates = @()
  $reusedProcessIds = @()
  foreach ($identity in $RememberedIdentities.Values) {
    $state = Get-FootprintTrackedProcessState $identity $ProcessRows
    switch ($state) {
      "matching" { $candidates += @($ProcessRows | Where-Object { [int]$_.ProcessId -eq [int]$identity.processId }) }
      "reused" { $reusedProcessIds += [int]$identity.processId }
      "lost" { $identityLost = $true }
    }
  }
  return [ordered]@{
    rootState = $rootState
    identityLost = $identityLost
    candidates = @($candidates | Sort-Object ProcessId)
    reusedProcessIds = @($reusedProcessIds | Sort-Object -Unique)
  }
}

function Get-FootprintForceKillCandidates {
  param([Parameter(Mandatory)][Collections.IDictionary]$RootIdentity, [AllowNull()][object[]]$ProcessRows)
  if ($null -eq $ProcessRows) { $ProcessRows = @(Get-FootprintProcessSnapshot) }
  $root = @($ProcessRows | Where-Object { [int]$_.ProcessId -eq [int]$RootIdentity.processId }) | Select-Object -First 1
  if ($null -eq $root -or -not (Test-FootprintProcessIdentity $root $RootIdentity.executablePath $RootIdentity.creationDate)) { return @() }
  return @(Get-FootprintDescendants ([int]$RootIdentity.processId) $ProcessRows -RootCreationDate $RootIdentity.creationDate)
}

function Update-FootprintTeardownPlan {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$RootIdentity,
    [Parameter(Mandatory)][Collections.Generic.Dictionary[string, object]]$KnownProcessIdentities,
    [Parameter(Mandatory)][AllowEmptyCollection()][object[]]$ProcessRows,
    [Parameter(Mandatory)][ref]$TreeObserved
  )
  $plan = Get-FootprintTeardownPlan $RootIdentity $KnownProcessIdentities $ProcessRows
  if (-not $TreeObserved.Value -and $plan.rootState -eq "matching") {
    # A recovered root must establish the same bounded descendant boundary as
    # the initial observation before its tree can be terminated or proven zero.
    Add-FootprintRememberedIdentities $KnownProcessIdentities @(
      Get-FootprintDescendants ([int]$RootIdentity.processId) $ProcessRows -RootCreationDate $RootIdentity.creationDate
    )
    $TreeObserved.Value = $true
    $plan = Get-FootprintTeardownPlan $RootIdentity $KnownProcessIdentities $ProcessRows
  }
  return $plan
}

function Invoke-FootprintTreeCore {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$RootIdentity,
    [Parameter(Mandatory)][Collections.Generic.Dictionary[string, object]]$KnownProcessIdentities,
    [ValidateRange(1, 30)][int]$GraceSeconds = 1,
    [Parameter(Mandatory)][scriptblock]$SnapshotProvider,
    # Terminates the process an identity names and returns $true, or returns
    # $false when that PID no longer names the identified process.
    [Parameter(Mandatory)][scriptblock]$StopExactProcess,
    [Parameter(Mandatory)][scriptblock]$UtcNow,
    [Parameter(Mandatory)][scriptblock]$Delay
  )
  $rootPid = [int]$RootIdentity.processId
  [void](Add-FootprintRememberedIdentity $KnownProcessIdentities ([pscustomobject]@{ ProcessId = $rootPid; ExecutablePath = $RootIdentity.executablePath; CreationDate = $RootIdentity.creationDate }))
  $reusedProcessIds = [Collections.Generic.HashSet[int]]::new()
  $treeObserved = $false
  $initialRows = @(& $SnapshotProvider)
  $plan = Update-FootprintTeardownPlan $RootIdentity $KnownProcessIdentities $initialRows ([ref]$treeObserved)
  foreach ($reusedProcessId in @($plan.reusedProcessIds)) { [void]$reusedProcessIds.Add([int]$reusedProcessId) }
  if ($plan.rootState -eq "matching") {
    try {
      $root = Open-FootprintExactProcess $RootIdentity
      if ($null -ne $root) {
        try {
          if ($root.MainWindowHandle -ne [IntPtr]::Zero) { [void]$root.CloseMainWindow() }
        } finally {
          $root.Dispose()
        }
      }
    } catch {
      # A graceful close request is best effort; the exact stop below still applies.
    }
  }
  $deadline = ([DateTime](& $UtcNow)).AddSeconds($GraceSeconds)
  do {
    $plan = Update-FootprintTeardownPlan $RootIdentity $KnownProcessIdentities @(& $SnapshotProvider) ([ref]$treeObserved)
    foreach ($reusedProcessId in @($plan.reusedProcessIds)) { [void]$reusedProcessIds.Add([int]$reusedProcessId) }
    if ($plan.candidates.Count -eq 0 -and -not $plan.identityLost) { return [ordered]@{ forced = $false; identityLost = [bool]$plan.identityLost; candidates = @(); remaining = @(); reusedProcessIds = @($reusedProcessIds | Sort-Object) } }
    & $Delay 100
  } while ([DateTime](& $UtcNow) -lt $deadline)
  $plan = Update-FootprintTeardownPlan $RootIdentity $KnownProcessIdentities @(& $SnapshotProvider) ([ref]$treeObserved)
  foreach ($reusedProcessId in @($plan.reusedProcessIds)) { [void]$reusedProcessIds.Add([int]$reusedProcessId) }
  $candidates = @($plan.candidates | Sort-Object @{ Expression = { [int]$_.ProcessId -eq $rootPid } }, @{ Expression = { [int]$_.ProcessId }; Descending = $true })
  $killed = @()
  foreach ($candidate in $candidates) {
    $currentPlan = Update-FootprintTeardownPlan $RootIdentity $KnownProcessIdentities @(& $SnapshotProvider) ([ref]$treeObserved)
    foreach ($reusedProcessId in @($currentPlan.reusedProcessIds)) { [void]$reusedProcessIds.Add([int]$reusedProcessId) }
    $current = @($currentPlan.candidates | Where-Object { [int]$_.ProcessId -eq [int]$candidate.ProcessId }) | Select-Object -First 1
    if ($null -eq $current -or -not (Test-FootprintProcessIdentity $current $candidate.ExecutablePath (ConvertTo-FootprintCreationIdentity $candidate.CreationDate))) { continue }
    $currentIdentity = ConvertTo-FootprintProcessIdentity $current
    try {
      $terminated = [bool](& $StopExactProcess $currentIdentity)
    } catch {
      $afterStopFailure = Update-FootprintTeardownPlan $RootIdentity $KnownProcessIdentities @(& $SnapshotProvider) ([ref]$treeObserved)
      foreach ($reusedProcessId in @($afterStopFailure.reusedProcessIds)) { [void]$reusedProcessIds.Add([int]$reusedProcessId) }
      $stillPresent = @($afterStopFailure.candidates | Where-Object {
          [int]$_.ProcessId -eq [int]$candidate.ProcessId -and
          (Test-FootprintProcessIdentity $_ $candidate.ExecutablePath (ConvertTo-FootprintCreationIdentity $candidate.CreationDate))
        }).Count -eq 1
      if ($stillPresent) { throw }
      continue
    }
    if (-not $terminated) {
      [void]$reusedProcessIds.Add([int]$current.ProcessId)
      continue
    }
    $killed += $currentIdentity
  }
  $postKillDeadline = ([DateTime](& $UtcNow)).AddSeconds($script:PostKillWaitSeconds)
  do {
    $remainingPlan = Update-FootprintTeardownPlan $RootIdentity $KnownProcessIdentities @(& $SnapshotProvider) ([ref]$treeObserved)
    foreach ($reusedProcessId in @($remainingPlan.reusedProcessIds)) { [void]$reusedProcessIds.Add([int]$reusedProcessId) }
    # An empty kill set is not exit proof while an identity is unobservable.
    # Keep observing within the existing deadline until terminal zero is proven.
    if ($remainingPlan.candidates.Count -eq 0 -and -not $remainingPlan.identityLost) { break }
    & $Delay 100
  } while ([DateTime](& $UtcNow) -lt $postKillDeadline)
  return [ordered]@{
    forced = $killed.Count -gt 0
    identityLost = [bool]$remainingPlan.identityLost
    candidates = @($candidates | ForEach-Object ProcessId)
    remaining = @($remainingPlan.candidates | ForEach-Object ProcessId)
    reusedProcessIds = @($reusedProcessIds | Sort-Object)
  }
}

function Open-FootprintExactProcess {
  param([Parameter(Mandatory)][Collections.IDictionary]$Identity)
  # The retained handle pins one process, so the creation-time check and any
  # later action on the returned object address that same process even if its
  # PID is reused in between. The caller disposes the returned object.
  $process = Get-Process -Id ([int]$Identity.processId) -ErrorAction SilentlyContinue
  if ($null -eq $process) { return $null }
  try {
    [void]$process.Handle
    $sameProcess = (ConvertTo-FootprintCreationIdentity $process.StartTime) -eq [string]$Identity.creationDate
  } catch {
    $process.Dispose()
    throw
  }
  if ($sameProcess) { return $process }
  $process.Dispose()
  return $null
}

function Stop-FootprintExactProcess {
  param([Parameter(Mandatory)][Collections.IDictionary]$Identity)
  $process = Open-FootprintExactProcess $Identity
  if ($null -eq $process) { return $false }
  try {
    $process.Kill()
    return $true
  } finally {
    $process.Dispose()
  }
}

function Stop-FootprintTree {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$RootIdentity,
    [Parameter(Mandatory)][Collections.Generic.Dictionary[string, object]]$KnownProcessIdentities,
    [ValidateRange(1, 30)][int]$GraceSeconds = 1
  )
  return Invoke-FootprintTreeCore `
    $RootIdentity `
    $KnownProcessIdentities `
    $GraceSeconds `
    { Get-FootprintProcessSnapshot } `
    { param([Collections.IDictionary]$Identity) Stop-FootprintExactProcess $Identity } `
    { [DateTime]::UtcNow } `
    { param([int]$Milliseconds) Start-Sleep -Milliseconds $Milliseconds }
}

Export-ModuleMember -Function @(
  "Add-FootprintRememberedIdentities",
  "Add-FootprintRememberedIdentity",
  "ConvertTo-FootprintProcessIdentity",
  "Get-FootprintDescendants",
  "Get-FootprintForceKillCandidates",
  "Get-FootprintLaunchIdentity",
  "Get-FootprintNativeProcessIdentity",
  "Get-FootprintProcessLimits",
  "Get-FootprintProcessSnapshot",
  "Get-FootprintRootIdentity",
  "Get-FootprintTeardownPlan",
  "New-FootprintIdentityStore",
  "Open-FootprintExactProcess",
  "Stop-FootprintExactProcess",
  "Stop-FootprintLaunchedProcessObject",
  "Stop-FootprintTree"
)
