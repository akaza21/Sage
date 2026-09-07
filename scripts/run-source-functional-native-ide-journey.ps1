[CmdletBinding()]
param(
  [string]$SageExecutable,
  [string]$SourceRoot,
  [string]$EvidenceRoot,
  [ValidateSet("native-ide", "acp04-live", "av01-preview")][string]$JourneyKind = "native-ide",
  [ValidateSet("starting-active", "health-failed", "exited")][string]$PreviewCase = "starting-active",
  [ValidateRange(15000, 180000)][int]$DeadlineMs = 90000,
  [ValidateSet("capture", "cleanup")][string]$ToolIdentityAction,
  [int]$ToolProcessId,
  [string]$ToolIdentityJson
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Test-FullyQualifiedPath([string]$Value) {
  if ([string]::IsNullOrWhiteSpace($Value) -or -not [IO.Path]::IsPathRooted($Value)) { return $false }
  try {
    $canonical = [IO.Path]::GetFullPath($Value)
    if ([string]::IsNullOrWhiteSpace($canonical)) { return $false }
    $trimmedValue = $Value.TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
    $trimmedCanonical = $canonical.TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
    if ($trimmedValue -match '(^|[\\/])\.\.?(?:[\\/]|$)') { return $false }
    return [string]::Equals($trimmedValue, $trimmedCanonical, [StringComparison]::OrdinalIgnoreCase)
  } catch {
    return $false
  }
}

function Require-AbsoluteFile([string]$Value, [string]$Label) {
  if (-not (Test-FullyQualifiedPath $Value)) { throw "$Label must be an absolute canonical path." }
  $item = Get-Item -LiteralPath $Value -Force
  if (-not $item -or $item.PSIsContainer) { throw "$Label must name a file." }
  return $item.FullName
}

function Require-PlainCanonicalFile([string]$Value, [string]$Label) {
  $resolved = Require-AbsoluteFile $Value $Label
  $current = $resolved
  while ($true) {
    $item = Get-Item -LiteralPath $current -Force
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw "$Label path contains a reparse point." }
    if (-not [string]::Equals($item.FullName, $current, [StringComparison]::OrdinalIgnoreCase)) { throw "$Label path is not canonical." }
    $parent = Split-Path -Parent $current
    if (-not $parent -or $parent -eq $current) { break }
    $current = $parent
  }
  return $resolved
}

function Get-BoundedLoopbackPort {
  $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
  try {
    $listener.Start()
    return ([Net.IPEndPoint]$listener.LocalEndpoint).Port
  } finally {
    $listener.Stop()
  }
}

function Assert-PlainContainedDirectory([string]$Root, [string]$Candidate) {
  $resolvedRoot = [IO.Path]::GetFullPath($Root)
  $current = [IO.Path]::GetFullPath($Candidate)
  do {
    $item = Get-Item -LiteralPath $current -Force
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw "Evidence path contains a reparse point." }
    if ([string]::Equals($current, $resolvedRoot, [StringComparison]::OrdinalIgnoreCase)) { return }
    $parent = Split-Path -Parent $current
    if ($parent -eq $current) { break }
    $current = $parent
  } while ($true)
  throw "Evidence path is not contained by SourceRoot\\local-evidence."
}

function Invoke-GitAt([string]$Repository, [string[]]$Arguments) {
  $value = & git -C $Repository @Arguments
  if ($LASTEXITCODE -ne 0) { throw "git $($Arguments -join ' ') failed." }
  $text = if ($null -eq $value) { "" } else { [string]$value }
  return $text.Trim()
}

function Invoke-Git([string[]]$Arguments) {
  return Invoke-GitAt -Repository $resolvedSource -Arguments $Arguments
}

function Initialize-JourneyFixture([string]$Fixture) {
  # The fixture deliberately begins with one tracked TypeScript diagnostic. The
  # CDP runner makes a further real editor edit before saving and opening Diff.
  [IO.File]::WriteAllText(
    (Join-Path $Fixture "journey.ts"),
    'export const SAGE_JOURNEY_SENTINEL: number = "diagnostic";',
    [Text.UTF8Encoding]::new($false)
  )
  [void](Invoke-GitAt -Repository $Fixture -Arguments @("init", "--quiet"))
  [void](Invoke-GitAt -Repository $Fixture -Arguments @("config", "user.name", "Sage journey fixture"))
  [void](Invoke-GitAt -Repository $Fixture -Arguments @("config", "user.email", "sage-journey-fixture@localhost"))
  [void](Invoke-GitAt -Repository $Fixture -Arguments @("add", "--", "journey.ts"))
  [void](Invoke-GitAt -Repository $Fixture -Arguments @("commit", "--quiet", "-m", "seed native journey fixture"))
  return [ordered]@{
    commit = Invoke-GitAt -Repository $Fixture -Arguments @("rev-parse", "HEAD")
    tree = Invoke-GitAt -Repository $Fixture -Arguments @("rev-parse", "HEAD^{tree}")
  }
}

function Resolve-FrontendPortContract([string]$SourceRoot) {
  $tauriConfig = Get-Content -LiteralPath (Join-Path $SourceRoot "src-tauri/tauri.conf.json") -Raw | ConvertFrom-Json
  $devUrl = $null
  if (-not [Uri]::TryCreate([string]$tauriConfig.build.devUrl, [UriKind]::Absolute, [ref]$devUrl) -or
      $devUrl.Scheme -ne "http" -or $devUrl.Host -ne "127.0.0.1" -or
      $devUrl.AbsolutePath -ne "/" -or $devUrl.Query -or $devUrl.Fragment -or $devUrl.UserInfo -or
      $devUrl.Port -lt 1024 -or $devUrl.Port -gt 65535) {
    throw "Tauri devUrl must name one exact loopback frontend port."
  }
  $vitePath = Join-Path $SourceRoot "vite.config.ts"
  $viteItem = Get-Item -LiteralPath $vitePath -Force
  if ($viteItem.PSIsContainer -or $viteItem.Length -gt 1MB) { throw "Vite config is invalid." }
  $viteText = Get-Content -LiteralPath $vitePath -Raw
  $matches = [regex]::Matches($viteText, '(?ms)^\s*server\s*:\s*\{\s*port\s*:\s*(?<port>\d{1,5})\s*,\s*strictPort\s*:\s*true\s*,')
  if ($matches.Count -ne 1) { throw "Vite must declare one strict frontend port." }
  $vitePort = [int]$matches[0].Groups["port"].Value
  if ($vitePort -ne $devUrl.Port) { throw "Tauri and Vite frontend ports do not match." }
  return [ordered]@{ port = $vitePort; origin = $devUrl.AbsoluteUri }
}

function Assert-FrontendPortAvailable([Collections.IDictionary]$Contract) {
  $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, [int]$Contract.port)
  try {
    $listener.ExclusiveAddressUse = $true
    $listener.Start()
  } catch {
    throw "The required frontend port is already occupied."
  } finally {
    $listener.Stop()
  }
}

function New-OwnedRuntime(
  [System.Diagnostics.Process]$Process,
  [string]$ExecutablePath,
  [object]$Identity
) {
  foreach ($property in @("processId", "executablePath", "creationDate")) {
    $hasProperty = $null -ne $Identity -and (
      ($Identity -is [Collections.IDictionary] -and $Identity.Contains($property)) -or
      ($null -ne $Identity.PSObject.Properties[$property])
    )
    if (-not $hasProperty) {
      throw "The launched process identity is incomplete."
    }
  }
  $expectedPath = [IO.Path]::GetFullPath($ExecutablePath)
  $expectedCreation = $Process.StartTime.ToUniversalTime().ToString(
    "yyyy-MM-ddTHH:mm:ss.fff'Z'",
    [Globalization.CultureInfo]::InvariantCulture
  )
  if ([int]$Identity.processId -ne [int]$Process.Id -or
      -not [string]::Equals(
        [IO.Path]::GetFullPath([string]$Identity.executablePath),
        $expectedPath,
        [StringComparison]::OrdinalIgnoreCase
      ) -or [string]$Identity.creationDate -ne $expectedCreation) {
    throw "The launched process identity does not match its exact process object."
  }
  $store = New-FootprintIdentityStore
  [void](Add-FootprintRememberedIdentity $store ([pscustomobject]@{
    ProcessId = $Identity.processId
    ExecutablePath = $Identity.executablePath
    CreationDate = $Identity.creationDate
  }))
  return [ordered]@{ process = $Process; identity = $Identity; store = $store }
}

function Write-LaunchIdentityQuarantine(
  [ValidateSet("sage", "vite")][string]$Role,
  [string]$EvidenceRoot,
  [int]$ObservedPid,
  [ValidateSet(
    "identity-unavailable",
    "path-mismatch",
    "process-exited",
    "resolver-pid-mismatch",
    "resolver-creation-mismatch"
  )][string]$FailureCategory,
  [ValidateRange(0, 100)][int]$AttemptCount
) {
  if (-not $EvidenceRoot -or -not (Test-Path -LiteralPath $EvidenceRoot -PathType Container)) { return }
  [ordered]@{
    schemaVersion = 2
    runtimeRole = $Role
    category = "identity-capture-failed"
    failureCategory = $FailureCategory
    observedPid = $ObservedPid
    attemptCount = $AttemptCount
  } | ConvertTo-Json -Compress | Set-Content `
      -LiteralPath (Join-Path $EvidenceRoot ("runtime-launch-quarantine-{0}.json" -f $Role)) `
      -NoNewline -Encoding utf8
}

function Stop-UnadmittedOwnedRuntime(
  [System.Diagnostics.Process]$Process,
  [string]$ExecutablePath,
  [ValidateSet("sage", "vite")][string]$Role,
  [string]$EvidenceRoot,
  [AllowNull()][Collections.Generic.Dictionary[string, object]]$RememberedIdentities,
  [AllowNull()][object[]]$ProcessRows
) {
  if ($null -eq $RememberedIdentities) {
    $RememberedIdentities = New-FootprintIdentityStore
  }
  try {
    $ProcessRows = @(Get-FootprintProcessSnapshot)
    foreach ($row in @(Get-FootprintDescendants ([int]$Process.Id) $ProcessRows)) {
      [void](Add-FootprintRememberedIdentity $RememberedIdentities $row)
    }
  } catch {
    $ProcessRows = $null
  }
  $cleanup = Stop-FootprintLaunchedProcessObject `
    -Process $Process `
    -ExpectedExecutablePath $ExecutablePath `
    -KnownProcessIdentities $RememberedIdentities `
    -ProcessRows $ProcessRows `
    -GraceMilliseconds 0 `
    -ForceWaitMilliseconds 5000
  return [ordered]@{
    terminalZero = [bool]$cleanup.exited -and [bool]$cleanup.treeBoundaryVerified
  }
}

function Capture-OwnedRuntimeOrQuarantine(
  [System.Diagnostics.Process]$Process,
  [string]$ExecutablePath,
  [ValidateSet("sage", "vite")][string]$Role,
  [string]$EvidenceRoot,
  [Parameter(Mandatory)][long]$AbsoluteDeadlineMilliseconds,
  [scriptblock]$IdentityResolver,
  [scriptblock]$Delay,
  [Parameter(Mandatory)][scriptblock]$MonotonicMilliseconds,
  [scriptblock]$CleanupUnadmitted
) {
  if ($AbsoluteDeadlineMilliseconds -lt 0) {
    throw "The absolute launch-identity deadline is invalid."
  }
  if ($null -eq $IdentityResolver) {
    $IdentityResolver = { param($Candidate, $ExpectedPath) Get-FootprintLaunchIdentity $Candidate $ExpectedPath }
  }
  if ($null -eq $Delay) {
    $Delay = { param($Milliseconds) Start-Sleep -Milliseconds $Milliseconds }
  }
  if ($null -eq $CleanupUnadmitted) {
    $CleanupUnadmitted = {
      param($Candidate, $ExpectedPath, $RuntimeRole, $Root, $Remembered, $Rows)
      Stop-UnadmittedOwnedRuntime $Candidate $ExpectedPath $RuntimeRole $Root $Remembered $Rows
    }
  }
  $captureDeadline = [long][Math]::Min(
    $AbsoluteDeadlineMilliseconds,
    [long](& $MonotonicMilliseconds) + 2000
  )
  $pollMilliseconds = 40
  $attemptCount = 0
  $failureCategory = "identity-unavailable"
  $cleanupPath = [IO.Path]::GetFullPath($ExecutablePath)
  $remembered = New-FootprintIdentityStore
  $lastSnapshot = $null
  while ([long](& $MonotonicMilliseconds) -lt $captureDeadline) {
    $attemptCount++
    $refreshSucceeded = $false
    try {
      $Process.Refresh()
      $refreshSucceeded = $true
    } catch {
      # A temporarily unavailable process object remains unadmitted and bounded.
    }
    if ($refreshSucceeded) {
      try {
        $snapshot = @(Get-FootprintProcessSnapshot)
        foreach ($row in @(Get-FootprintDescendants ([int]$Process.Id) $snapshot)) {
          [void](Add-FootprintRememberedIdentity $remembered $row)
        }
        $lastSnapshot = $snapshot
      } catch {
        # Cleanup retains any identities remembered by an earlier attempt.
      }
      $hasExited = $false
      try {
        $hasExited = [bool]$Process.HasExited
      } catch {
        $refreshSucceeded = $false
      }
      if ($refreshSucceeded -and $hasExited) {
        $failureCategory = "process-exited"
        break
      }
    }
    if ($refreshSucceeded) {
      try {
        $observedPath = [IO.Path]::GetFullPath([string]$Process.Path)
        if (-not [string]::Equals(
            $observedPath,
            [IO.Path]::GetFullPath($ExecutablePath),
            [StringComparison]::OrdinalIgnoreCase
          )) {
          $failureCategory = "path-mismatch"
          $cleanupPath = $observedPath
          break
        }
        $cleanupPath = $observedPath
      } catch {
        # Get-FootprintLaunchIdentity remains the sole admission authority.
      }
      $identity = $null
      try {
        $identity = & $IdentityResolver $Process $ExecutablePath
      } catch {
        # Property visibility failures are retried inside the absolute window.
      }
      if ($null -ne $identity) {
        $identityComplete = $true
        foreach ($property in @("processId", "executablePath", "creationDate")) {
          $hasProperty = (
            ($identity -is [Collections.IDictionary] -and $identity.Contains($property)) -or
            ($null -ne $identity.PSObject.Properties[$property])
          )
          if (-not $hasProperty) { $identityComplete = $false }
        }
        if ($identityComplete) {
          $resolvedPid = $null
          try { $resolvedPid = [int]$identity.processId } catch { }
          if ($null -eq $resolvedPid -or $resolvedPid -ne [int]$Process.Id) {
            $failureCategory = "resolver-pid-mismatch"
            break
          }
          $resolverPath = $null
          try {
            $resolverPath = [IO.Path]::GetFullPath([string]$identity.executablePath)
          } catch { }
          if ($null -eq $resolverPath -or -not [string]::Equals(
              $resolverPath,
              [IO.Path]::GetFullPath($ExecutablePath),
              [StringComparison]::OrdinalIgnoreCase
            )) {
            $failureCategory = "path-mismatch"
            break
          }
          $expectedCreation = $null
          try {
            $expectedCreation = $Process.StartTime.ToUniversalTime().ToString(
              "yyyy-MM-ddTHH:mm:ss.fff'Z'",
              [Globalization.CultureInfo]::InvariantCulture
            )
          } catch { }
          if ($null -ne $expectedCreation) {
            if ([string]$identity.creationDate -ne $expectedCreation) {
              $failureCategory = "resolver-creation-mismatch"
              break
            }
            return New-OwnedRuntime $Process $ExecutablePath $identity
          }
        }
      }
    }
    $remaining = $captureDeadline - [long](& $MonotonicMilliseconds)
    if ($remaining -le 0) { break }
    & $Delay ([int][Math]::Min($pollMilliseconds, $remaining))
  }
  $launchQuarantineRetained = $true
  try {
    Write-LaunchIdentityQuarantine $Role $EvidenceRoot ([int]$Process.Id) $failureCategory $attemptCount
  } catch {
    $launchQuarantineRetained = $false
  }
  try {
    $cleanup = & $CleanupUnadmitted `
      $Process $cleanupPath $Role $EvidenceRoot $remembered $lastSnapshot
    $hasTerminalZero = $null -ne $cleanup -and (
      ($cleanup -is [Collections.IDictionary] -and $cleanup.Contains("terminalZero")) -or
      ($null -ne $cleanup.PSObject.Properties["terminalZero"])
    )
    if (-not $hasTerminalZero -or -not [bool]$cleanup.terminalZero) {
      throw "cleanup did not prove terminal zero"
    }
  } catch {
    try { Write-RuntimeCleanupQuarantine $Role $EvidenceRoot $null $null } catch { }
    throw "The $Role runtime identity could not be captured and cleanup could not be proven."
  }
  if (-not $launchQuarantineRetained) {
    throw "The $Role runtime reached terminal zero, but its launch quarantine could not be retained."
  }
  throw "The $Role runtime identity could not be captured; the launched runtime reached terminal zero."
}

function Update-OwnedRuntimeTree([Collections.IDictionary]$Runtime) {
  $rows = @(Get-FootprintProcessSnapshot)
  [void](Get-FootprintRootIdentity `
      -RootProcessId ([int]$Runtime.identity.processId) `
      -ExpectedExecutablePath ([string]$Runtime.identity.executablePath) `
      -ExpectedCreationDate ([string]$Runtime.identity.creationDate) `
      -ProcessRows $rows)
  foreach ($row in @(Get-FootprintDescendants ([int]$Runtime.identity.processId) $rows)) {
    [void](Add-FootprintRememberedIdentity $Runtime.store $row)
  }
}

function Get-FrontendListenerIdentity([Collections.IDictionary]$Contract) {
  $listeners = @(Get-NetTCPConnection -LocalAddress "127.0.0.1" -LocalPort ([int]$Contract.port) -State Listen -ErrorAction SilentlyContinue)
  if ($listeners.Count -gt 1) { throw "The frontend listener is ambiguous." }
  if ($listeners.Count -eq 0) { return $null }
  $rows = @(Get-FootprintProcessSnapshot | Where-Object { [int]$_.ProcessId -eq [int]$listeners[0].OwningProcess })
  if ($rows.Count -ne 1) { return $null }
  return ConvertTo-FootprintProcessIdentity $rows[0]
}

function Test-OwnedRuntimeProcessIdentity(
  [Collections.IDictionary]$Runtime,
  [Collections.IDictionary]$Identity
) {
  return @($Runtime.store.Values | Where-Object {
      [int]$_.processId -eq [int]$Identity.processId -and
      [string]::Equals(
        [IO.Path]::GetFullPath([string]$_.executablePath),
        [IO.Path]::GetFullPath([string]$Identity.executablePath),
        [StringComparison]::OrdinalIgnoreCase
      ) -and [string]$_.creationDate -eq [string]$Identity.creationDate
    }).Count -eq 1
}

function Test-SameProcessIdentity(
  [Collections.IDictionary]$Before,
  [Collections.IDictionary]$After
) {
  return [int]$Before.processId -eq [int]$After.processId -and
    [string]::Equals(
      [IO.Path]::GetFullPath([string]$Before.executablePath),
      [IO.Path]::GetFullPath([string]$After.executablePath),
      [StringComparison]::OrdinalIgnoreCase
    ) -and [string]$Before.creationDate -eq [string]$After.creationDate
}

function Test-FrontendHttpReady([Collections.IDictionary]$Contract, [int]$TimeoutMilliseconds) {
  $request = [Net.WebRequest]::Create([string]$Contract.origin)
  $request.Method = "GET"
  $request.Proxy = $null
  $request.Timeout = $TimeoutMilliseconds
  $request.ReadWriteTimeout = $TimeoutMilliseconds
  $response = $null
  try {
    $response = $request.GetResponse()
    $status = [int]$response.StatusCode
    return $status -ge 200 -and $status -lt 400
  } catch {
    return $false
  } finally {
    if ($null -ne $response) { $response.Close() }
    $request.Abort()
  }
}

function Wait-OwnedFrontendReady(
  [Collections.IDictionary]$Runtime,
  [Collections.IDictionary]$Contract,
  [DateTime]$StartupDeadline,
  [AllowNull()][scriptblock]$ListenerProbe,
  [AllowNull()][scriptblock]$HttpProbe
) {
  if ($null -eq $ListenerProbe) { $ListenerProbe = { param($value) Get-FrontendListenerIdentity $value } }
  if ($null -eq $HttpProbe) { $HttpProbe = { param($value, $timeout) Test-FrontendHttpReady $value $timeout } }
  $unownedListenerSince = $null
  do {
    $Runtime.process.Refresh()
    if ($Runtime.process.HasExited) { throw "The owned frontend exited before readiness." }
    Update-OwnedRuntimeTree $Runtime
    $listenerBefore = & $ListenerProbe $Contract
    if ($null -ne $listenerBefore) {
      if (-not (Test-OwnedRuntimeProcessIdentity $Runtime $listenerBefore)) {
        if ($null -eq $unownedListenerSince) { $unownedListenerSince = [DateTime]::UtcNow }
        if (([DateTime]::UtcNow - $unownedListenerSince).TotalMilliseconds -ge 1000) {
          throw "The frontend listener is not owned by the captured runtime tree."
        }
        Start-Sleep -Milliseconds 100
        continue
      }
      $unownedListenerSince = $null
    }
    $remaining = [int][Math]::Max(1, [Math]::Min(500, ($StartupDeadline - [DateTime]::UtcNow).TotalMilliseconds))
    if ($null -ne $listenerBefore -and (& $HttpProbe $Contract $remaining) -and [DateTime]::UtcNow -le $StartupDeadline) {
      $listenerAfter = & $ListenerProbe $Contract
      if ($null -eq $listenerAfter -or
          -not (Test-OwnedRuntimeProcessIdentity $Runtime $listenerAfter) -or
          -not (Test-SameProcessIdentity $listenerBefore $listenerAfter)) {
        throw "The frontend listener identity changed during readiness."
      }
      return [ordered]@{ mode = "tauri-dev-split"; frontendReady = $true }
    }
    if ([DateTime]::UtcNow -lt $StartupDeadline) { Start-Sleep -Milliseconds 100 }
  } while ([DateTime]::UtcNow -lt $StartupDeadline)
  throw "The owned frontend did not become ready within its startup deadline."
}

function Start-OwnedVite(
  [string]$SourceRoot,
  [Collections.IDictionary]$Contract,
  [string]$EvidenceRoot,
  [long]$StartupDeadlineMilliseconds,
  [scriptblock]$MonotonicMilliseconds
) {
  Assert-FrontendPortAvailable $Contract
  $npm = @(Get-Command npm.cmd -CommandType Application -ErrorAction Stop) | Select-Object -First 1
  $command = @(Get-Command cmd.exe -CommandType Application -ErrorAction Stop) | Select-Object -First 1
  $npmPath = Require-AbsoluteFile $npm.Source "npm launcher"
  $commandPath = Require-AbsoluteFile $command.Source "command interpreter"
  $commandLine = '/d /s /c ""{0}" run dev -- --host 127.0.0.1 --port {1}"' -f $npmPath, $Contract.port
  $process = Start-Process -FilePath $commandPath -ArgumentList $commandLine -WorkingDirectory $SourceRoot -WindowStyle Hidden -PassThru
  return Capture-OwnedRuntimeOrQuarantine `
    $process $commandPath "vite" $EvidenceRoot `
    $StartupDeadlineMilliseconds $null $null $MonotonicMilliseconds
}

function Write-RuntimeCleanupQuarantine(
  [ValidateSet("sage", "vite")][string]$Role,
  [string]$EvidenceRoot,
  [object]$IdentityLost,
  [object]$RemainingCount
) {
  if (-not $EvidenceRoot -or -not (Test-Path -LiteralPath $EvidenceRoot -PathType Container)) { return }
  $record = [ordered]@{
    schemaVersion = 1
    runtimeRole = $Role
    category = "cleanup-unproven"
    identityLost = $IdentityLost
    remainingCount = $RemainingCount
  }
  $target = Join-Path $EvidenceRoot ("runtime-cleanup-quarantine-{0}.json" -f $Role)
  $record | ConvertTo-Json -Compress | Set-Content -LiteralPath $target -NoNewline -Encoding utf8
}

function Stop-OwnedRuntimeTree(
  [ValidateSet("sage", "vite")][string]$Role,
  [AllowNull()][Collections.IDictionary]$Runtime,
  [string]$EvidenceRoot
) {
  if ($null -eq $Runtime) { return [ordered]@{ role = $Role; identityLost = $false; remainingCount = 0 } }
  try { Update-OwnedRuntimeTree $Runtime } catch { }
  try {
    $teardown = Stop-FootprintTree $Runtime.identity $Runtime.store 2
  } catch {
    Write-RuntimeCleanupQuarantine $Role $EvidenceRoot $null $null
    throw "The $Role runtime cleanup could not be proven."
  }
  $remainingCount = @($teardown.remaining).Count
  if ($teardown.identityLost -or $remainingCount -ne 0) {
    Write-RuntimeCleanupQuarantine $Role $EvidenceRoot ([bool]$teardown.identityLost) $remainingCount
    throw "The $Role runtime cleanup was not terminal-zero."
  }
  return [ordered]@{ role = $Role; identityLost = $false; remainingCount = 0 }
}

function Publish-PendingJourneyReceipt(
  [string]$PendingReceipt,
  [string]$FinalReceipt,
  [Collections.IDictionary]$SageCleanup,
  [Collections.IDictionary]$ViteCleanup
) {
  foreach ($result in @($SageCleanup, $ViteCleanup)) {
    if ($null -eq $result -or $result.identityLost -or [int]$result.remainingCount -ne 0) {
      throw "A pending journey receipt requires terminal-zero runtime cleanup."
    }
  }
  if (-not (Test-Path -LiteralPath $PendingReceipt -PathType Leaf) -or (Test-Path -LiteralPath $FinalReceipt)) {
    throw "Pending journey receipt promotion preconditions failed."
  }
  [IO.File]::Move($PendingReceipt, $FinalReceipt)
}

function Assert-BoundedNativeLog([string]$Path, [string]$Label) {
  if (-not (Test-FullyQualifiedPath $Path)) { throw "$Label path must be absolute and canonical." }
  $item = Get-Item -LiteralPath $Path -Force
  if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw "$Label must be a plain file."
  }
  if ($item.Length -gt 1MB) { throw "$Label exceeded its byte limit." }
  return [ordered]@{
    bytes = [long]$item.Length
    sha256 = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
  }
}

function Write-Acp04ProfileIdentity(
  [string]$Path,
  [string]$ProfileRoot,
  [string]$ProfileIdentitySha256
) {
  $payload = [ordered]@{
    schemaVersion = 1
    profileRoot = $ProfileRoot
    profileIdentitySha256 = $ProfileIdentitySha256
  } | ConvertTo-Json -Compress
  [IO.File]::WriteAllText($Path, $payload, [Text.UTF8Encoding]::new($false))
}

function Invoke-ControlledNodeCollector(
  [string]$Node,
  [string[]]$Arguments,
  [ValidateSet("native-ide", "acp04-live", "av01-preview")][string]$JourneyKind = "native-ide"
) {
  $codeLoadingNames = [Collections.Generic.HashSet[string]]::new(
    [StringComparer]::OrdinalIgnoreCase
  )
  foreach ($name in @(
    "NODE_OPTIONS",
    "NODE_PATH",
    "NODE_PRESERVE_SYMLINKS",
    "NODE_PRESERVE_SYMLINKS_MAIN",
    "NODE_REPL_EXTERNAL_MODULE"
  )) { [void]$codeLoadingNames.Add($name) }
  $snapshot = [Collections.Generic.List[object]]::new()
  $snapshotBytes = 0
  foreach ($entry in [Environment]::GetEnvironmentVariables("Process").GetEnumerator()) {
    $name = [string]$entry.Key
    if (-not ($name.StartsWith("NODE_", [StringComparison]::OrdinalIgnoreCase) -or
              $name.StartsWith("PLAYWRIGHT_", [StringComparison]::OrdinalIgnoreCase) -or
              $name.StartsWith("PW_", [StringComparison]::OrdinalIgnoreCase))) { continue }
    $value = [string]$entry.Value
    $snapshotBytes += [Text.Encoding]::UTF8.GetByteCount($name)
    $snapshotBytes += [Text.Encoding]::UTF8.GetByteCount($value)
    if ($snapshot.Count -ge 256 -or $name.Length -gt 256 -or
        $value.Length -gt 32767 -or $snapshotBytes -gt 1MB) {
      throw "Collector environment snapshot exceeds its bound."
    }
    [void]$snapshot.Add([pscustomobject]@{ name = $name; value = $value })
    if ($codeLoadingNames.Contains($name) -and $value.Length -ne 0) {
      throw "Collector code-loading environment is not allowed."
    }
  }
  $exitCode = -1
  try {
    foreach ($entry in $snapshot) {
      [Environment]::SetEnvironmentVariable($entry.name, $null, "Process")
    }
    & $Node @Arguments
    $exitCode = $LASTEXITCODE
  } finally {
    foreach ($entry in [Environment]::GetEnvironmentVariables("Process").GetEnumerator()) {
      $name = [string]$entry.Key
      if ($name.StartsWith("NODE_", [StringComparison]::OrdinalIgnoreCase) -or
          $name.StartsWith("PLAYWRIGHT_", [StringComparison]::OrdinalIgnoreCase) -or
          $name.StartsWith("PW_", [StringComparison]::OrdinalIgnoreCase)) {
        [Environment]::SetEnvironmentVariable($name, $null, "Process")
      }
    }
    foreach ($entry in $snapshot) {
      [Environment]::SetEnvironmentVariable($entry.name, $entry.value, "Process")
    }
  }
  if ($exitCode -ne 0) {
    if ($JourneyKind -eq "acp04-live") {
      throw "ACP-04 collector failed; inspect machine-failure.json under EvidenceRoot."
    }
    if ($JourneyKind -eq "av01-preview") {
      throw "AV01 Preview collector failed; inspect machine-failure.json under EvidenceRoot."
    }
    throw "Native IDE journey runner failed; inspect machine-failure.json under EvidenceRoot."
  }
}

if ($ToolIdentityAction) {
  Import-Module (Join-Path $PSScriptRoot "footprint/ProcessIdentity.psm1") -Force
  if ($ToolIdentityAction -eq "capture") {
    $rows = @(Get-FootprintProcessSnapshot)
    $rootRow = @($rows | Where-Object { [int]$_.ProcessId -eq $ToolProcessId }) | Select-Object -First 1
    if ($null -eq $rootRow) { throw "Tool root process identity was unavailable." }
    $root = ConvertTo-FootprintProcessIdentity $rootRow
    $remembered = @(Get-FootprintDescendants $ToolProcessId $rows | ForEach-Object { ConvertTo-FootprintProcessIdentity $_ })
    [ordered]@{ processId = $root.processId; executablePath = $root.executablePath; creationDate = $root.creationDate; remembered = $remembered } | ConvertTo-Json -Depth 5 -Compress
    exit 0
  }
  $owned = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($ToolIdentityJson)) | ConvertFrom-Json
  $store = New-FootprintIdentityStore
  foreach ($item in @($owned.remembered)) { [void](Add-FootprintRememberedIdentity $store ([pscustomobject]@{ ProcessId = $item.processId; ExecutablePath = $item.executablePath; CreationDate = $item.creationDate })) }
  $root = [ordered]@{ processId = [int]$owned.processId; executablePath = [string]$owned.executablePath; creationDate = [string]$owned.creationDate }
  Stop-FootprintTree $root $store 1 | ConvertTo-Json -Depth 5 -Compress
  exit 0
}

if (-not $SourceRoot) { $SourceRoot = Split-Path -Parent $PSCommandPath }
$resolvedExecutable = Require-AbsoluteFile $SageExecutable "SageExecutable"
$preLaunchExecutableBytes = (Get-Item -LiteralPath $resolvedExecutable -Force).Length
$preLaunchExecutableSha256 = (Get-FileHash -LiteralPath $resolvedExecutable -Algorithm SHA256).Hash.ToLowerInvariant()
if (-not (Test-FullyQualifiedPath $SourceRoot)) { throw "SourceRoot must be an absolute canonical path." }
$resolvedSource = (Get-Item -LiteralPath $SourceRoot -Force).FullName
$gitMarker = Get-Item -LiteralPath (Join-Path $resolvedSource ".git") -Force -ErrorAction SilentlyContinue
if ($null -eq $gitMarker) { throw "SourceRoot must be a Git checkout." }
$node = Require-PlainCanonicalFile (Get-Command node -ErrorAction Stop).Source "node"
$preLaunchNodeBytes = (Get-Item -LiteralPath $node -Force).Length
$preLaunchNodeSha256 = (Get-FileHash -LiteralPath $node -Algorithm SHA256).Hash.ToLowerInvariant()
$av01PowerShell = $null
$av01PowerShellBytes = $null
$av01PowerShellSha256 = $null
if ($JourneyKind -eq "av01-preview") {
  $av01PowerShell = Require-PlainCanonicalFile (Join-Path $PSHOME "powershell.exe") "AV01 Preview helper runtime"
  $av01PowerShellBytes = (Get-Item -LiteralPath $av01PowerShell -Force).Length
  $av01PowerShellSha256 = (Get-FileHash -LiteralPath $av01PowerShell -Algorithm SHA256).Hash.ToLowerInvariant()
}
$runnerName = if ($JourneyKind -eq "acp04-live") {
  "source-functional-acp04-live-probe.mjs"
} elseif ($JourneyKind -eq "av01-preview") {
  "source-functional-av01-preview-probe.mjs"
} else {
  "source-functional-native-ide-journey.mjs"
}
$runner = Require-AbsoluteFile (Join-Path $PSScriptRoot $runnerName) "journey runner"
$finalizer = $null
if ($JourneyKind -eq "acp04-live") {
  $finalizer = Require-AbsoluteFile (Join-Path $PSScriptRoot "source-functional-acp04-finalizer.mjs") "ACP-04 finalizer"
} elseif ($JourneyKind -eq "av01-preview") {
  $finalizer = Require-AbsoluteFile (Join-Path $PSScriptRoot "source-functional-av01-preview-finalizer.mjs") "AV01 Preview finalizer"
}
$codex = $null
$codexBytes = $null
$codexSha256 = $null
$gitExecutable = $null
$gitBytes = $null
$gitSha256 = $null
if ($JourneyKind -eq "acp04-live" -or $JourneyKind -eq "av01-preview") {
  $codexCommand = @(Get-Command codex.cmd -CommandType Application -ErrorAction Stop) | Select-Object -First 1
  $codex = Require-PlainCanonicalFile $codexCommand.Source "Codex launcher"
  $codexBytes = (Get-Item -LiteralPath $codex -Force).Length
  $codexSha256 = (Get-FileHash -LiteralPath $codex -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($JourneyKind -eq "av01-preview") {
    $gitExecutable = Require-PlainCanonicalFile (Get-Command git -CommandType Application -ErrorAction Stop).Source "Git"
    $gitBytes = (Get-Item -LiteralPath $gitExecutable -Force).Length
    $gitSha256 = (Get-FileHash -LiteralPath $gitExecutable -Algorithm SHA256).Hash.ToLowerInvariant()
  }
}
Import-Module (Join-Path $PSScriptRoot "footprint/ProcessIdentity.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "footprint/DisposableProfile.psm1") -Force

$status = Invoke-Git @("status", "--porcelain=v1", "--untracked-files=all")
if ($status) { throw "SourceRoot is dirty; a native evidence journey requires an exact source tree." }
$commit = Invoke-Git @("rev-parse", "HEAD")
$tree = Invoke-Git @("rev-parse", "HEAD^{tree}")
if ($commit -notmatch "^[a-f0-9]{40,64}$" -or $tree -notmatch "^[a-f0-9]{40,64}$") { throw "Git did not return immutable source identity." }
$frontendContract = Resolve-FrontendPortContract $resolvedSource

if (-not $EvidenceRoot) {
  $stamp = [DateTime]::UtcNow.ToString("yyyyMMddTHHmmssZ")
  $journeyLabel = if ($JourneyKind -eq "acp04-live") { "acp04-live" } elseif ($JourneyKind -eq "av01-preview") { "av01-preview" } else { "native-ide" }
  $EvidenceRoot = Join-Path $resolvedSource "local-evidence\BETA-0001\source-functional\$journeyLabel-$stamp"
}
if (-not (Test-FullyQualifiedPath $EvidenceRoot)) { throw "EvidenceRoot must be an absolute canonical path." }
if (Test-Path -LiteralPath $EvidenceRoot) { throw "EvidenceRoot must not already exist." }
$allowedEvidence = [IO.Path]::GetFullPath((Join-Path $resolvedSource "local-evidence"))
$resolvedEvidence = [IO.Path]::GetFullPath($EvidenceRoot)
if (-not $resolvedEvidence.StartsWith($allowedEvidence + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw "EvidenceRoot must be inside SourceRoot\\local-evidence." }

$parent = Split-Path -Parent $EvidenceRoot
[void](New-Item -ItemType Directory -Path $parent -Force)
[void](New-Item -ItemType Directory -Path $EvidenceRoot)
Assert-PlainContainedDirectory $allowedEvidence $resolvedEvidence
$fixture = Join-Path $EvidenceRoot "fixture-project"
[void](New-Item -ItemType Directory -Path $fixture)
$profileIdentity = New-C0BDisposableProfile $EvidenceRoot "$($JourneyKind.Replace('_', '-'))-profile"
$profile = $profileIdentity.root
$profileIdentityArtifact = $null
if ($JourneyKind -eq "acp04-live" -or $JourneyKind -eq "av01-preview") {
  $profileIdentityArtifact = Join-Path $EvidenceRoot "acp04-profile-identity.json"
  Write-Acp04ProfileIdentity $profileIdentityArtifact $profile $profileIdentity.identitySha256
}

$fixtureIdentity = if ($JourneyKind -eq "av01-preview") {
  $fixtureBuilder = Require-AbsoluteFile (Join-Path $PSScriptRoot "source-functional-av01-preview-fixture.mjs") "AV01 Preview fixture builder"
  Invoke-ControlledNodeCollector $node @($fixtureBuilder, "--fixture", $fixture, "--preview-case", $PreviewCase) $JourneyKind
  $identity = Get-Content -LiteralPath (Join-Path $fixture "fixture-identity.json") -Raw | ConvertFrom-Json
  $names = @($identity.files | ForEach-Object { [string]$_.name } | Sort-Object)
  if (($names -join "`0") -ne ((@("av01-preview-case.json", "package.json", "preview-fixture.mjs") | Sort-Object) -join "`0")) {
    throw "AV01 Preview fixture identity does not describe its exact bounded files."
  }
  [void](Invoke-GitAt -Repository $fixture -Arguments @("init", "--quiet"))
  [void](Invoke-GitAt -Repository $fixture -Arguments @("config", "user.name", "Sage AV01 fixture"))
  [void](Invoke-GitAt -Repository $fixture -Arguments @("config", "user.email", "sage-av01-fixture@localhost"))
  [void](Invoke-GitAt -Repository $fixture -Arguments @("add", "--", "package.json", "av01-preview-case.json", "preview-fixture.mjs", "fixture-identity.json"))
  [void](Invoke-GitAt -Repository $fixture -Arguments @("commit", "--quiet", "-m", "seed AV01 Preview fixture"))
  [IO.File]::WriteAllText(
    (Join-Path $EvidenceRoot "av01-preview-fixture-git.json"),
    (([ordered]@{ commit = Invoke-GitAt -Repository $fixture -Arguments @("rev-parse", "HEAD"); tree = Invoke-GitAt -Repository $fixture -Arguments @("rev-parse", "HEAD^{tree}") } | ConvertTo-Json -Compress) + "`n"),
    [Text.UTF8Encoding]::new($false)
  )
  $identity
} else {
  Initialize-JourneyFixture $fixture
}
if ($JourneyKind -ne "av01-preview") {
  $fixtureIdentity |
    ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $fixture "fixture-identity.json") -NoNewline -Encoding utf8
}

$port = Get-BoundedLoopbackPort
$previous = @{
  profile = [Environment]::GetEnvironmentVariable("SAGE_DISPOSABLE_PROFILE_ROOT", "Process")
  project = [Environment]::GetEnvironmentVariable("SAGE_PROJECT_ROOT", "Process")
  webview = [Environment]::GetEnvironmentVariable("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "Process")
}
$startupWindowMilliseconds = [int][Math]::Min(30000, $DeadlineMs)
$startupDeadline = [DateTime]::UtcNow.AddMilliseconds($startupWindowMilliseconds)
$script:startupClock = [Diagnostics.Stopwatch]::StartNew()
$monotonicMilliseconds = { return [long]$script:startupClock.ElapsedMilliseconds }
$startupDeadlineMilliseconds = [long](& $monotonicMilliseconds) + $startupWindowMilliseconds
$viteRuntime = $null
$sageRuntime = $null
$runtimeReceipt = $null
$journeyFailure = $null
$cleanupFailures = @()
$pendingReceipt = Join-Path $EvidenceRoot "machine-receipt.pending.json"
$sageCleanup = $null
$viteCleanup = $null
$sageStdoutLog = Join-Path $EvidenceRoot "sage.stdout.log"
$sageStderrLog = Join-Path $EvidenceRoot "sage.stderr.log"
try {
  $viteRuntime = Start-OwnedVite `
    $resolvedSource $frontendContract $EvidenceRoot `
    $startupDeadlineMilliseconds $monotonicMilliseconds
  $runtimeReceipt = Wait-OwnedFrontendReady $viteRuntime $frontendContract $startupDeadline
  [Environment]::SetEnvironmentVariable("SAGE_DISPOSABLE_PROFILE_ROOT", $profile, "Process")
  [Environment]::SetEnvironmentVariable("SAGE_PROJECT_ROOT", $fixture, "Process")
  [Environment]::SetEnvironmentVariable("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--remote-debugging-address=127.0.0.1 --remote-debugging-port=$port", "Process")
  if ($JourneyKind -eq "acp04-live" -or $JourneyKind -eq "av01-preview") {
    $process = Start-Process -FilePath $resolvedExecutable `
      -WorkingDirectory (Split-Path -Parent $resolvedExecutable) `
      -RedirectStandardOutput $sageStdoutLog `
      -RedirectStandardError $sageStderrLog `
      -PassThru
  } else {
    $process = Start-Process -FilePath $resolvedExecutable `
      -WorkingDirectory (Split-Path -Parent $resolvedExecutable) -PassThru
  }
  $sageRuntime = Capture-OwnedRuntimeOrQuarantine `
    $process $resolvedExecutable "sage" $EvidenceRoot `
    $startupDeadlineMilliseconds $null $null $monotonicMilliseconds
  if ($JourneyKind -eq "acp04-live") {
    $arguments = @(
      $runner, "--port", "$port", "--output", $EvidenceRoot,
      "--source-commit", $commit, "--source-tree", $tree,
      "--executable", $resolvedExecutable, "--source-root", $resolvedSource,
      "--executable-bytes", "$preLaunchExecutableBytes", "--executable-sha256", $preLaunchExecutableSha256,
      "--node-bytes", "$preLaunchNodeBytes", "--node-sha256", $preLaunchNodeSha256,
      "--codex-path", $codex, "--codex-bytes", "$codexBytes", "--codex-sha256", $codexSha256,
      "--git-path", $gitExecutable, "--git-bytes", "$gitBytes", "--git-sha256", $gitSha256,
      "--profile-root", $profile, "--profile-identity-artifact", $profileIdentityArtifact,
      "--sage-stdout-log", $sageStdoutLog, "--sage-stderr-log", $sageStderrLog,
      "--sage-process-id", "$($sageRuntime.identity.processId)",
      "--sage-process-created", "$($sageRuntime.identity.creationDate)",
      "--expected-origin", $frontendContract.origin,
      "--runtime-mode", $runtimeReceipt.mode, "--frontend-ready", "$($runtimeReceipt.frontendReady)".ToLowerInvariant(),
      "--receipt-name", (Split-Path -Leaf $pendingReceipt),
      "--deadline-ms", "$DeadlineMs", "--mode", "two-turn-cancel-restart"
    )
  } elseif ($JourneyKind -eq "av01-preview") {
    $fixtureProcessHelper = Require-AbsoluteFile (Join-Path $PSScriptRoot "source-functional-av01-preview-process.ps1") "AV01 Preview process helper"
    $arguments = @(
      $runner, "--port", "$port", "--output", $EvidenceRoot,
      "--source-commit", $commit, "--source-tree", $tree,
      "--executable", $resolvedExecutable, "--fixture", $fixture,
      "--fixture-identity", (Join-Path $fixture "fixture-identity.json"), "--fixture-process-helper", $fixtureProcessHelper, "--preview-case", $PreviewCase,
      "--source-root", $resolvedSource,
      "--executable-bytes", "$preLaunchExecutableBytes", "--executable-sha256", $preLaunchExecutableSha256,
      "--node-bytes", "$preLaunchNodeBytes", "--node-sha256", $preLaunchNodeSha256,
      "--powershell-path", $av01PowerShell, "--powershell-bytes", "$av01PowerShellBytes", "--powershell-sha256", $av01PowerShellSha256,
      "--codex-path", $codex, "--codex-bytes", "$codexBytes", "--codex-sha256", $codexSha256,
      "--profile-root", $profile, "--profile-identity-artifact", $profileIdentityArtifact,
      "--sage-stdout-log", $sageStdoutLog, "--sage-stderr-log", $sageStderrLog,
      "--sage-process-id", "$($sageRuntime.identity.processId)",
      "--sage-process-created", "$($sageRuntime.identity.creationDate)",
      "--expected-origin", $frontendContract.origin,
      "--runtime-mode", $runtimeReceipt.mode, "--frontend-ready", "$($runtimeReceipt.frontendReady)".ToLowerInvariant(),
      "--receipt-name", (Split-Path -Leaf $pendingReceipt),
      "--deadline-ms", "$DeadlineMs"
    )
  } else {
    $arguments = @(
      $runner, "--port", "$port", "--output", $EvidenceRoot,
      "--source-commit", $commit, "--source-tree", $tree,
      "--executable", $resolvedExecutable, "--fixture", $fixture,
      "--source-root", $resolvedSource,
      "--executable-bytes", "$preLaunchExecutableBytes", "--executable-sha256", $preLaunchExecutableSha256,
      "--node-bytes", "$preLaunchNodeBytes", "--node-sha256", $preLaunchNodeSha256,
      "--expected-origin", $frontendContract.origin,
      "--runtime-mode", $runtimeReceipt.mode, "--frontend-ready", "$($runtimeReceipt.frontendReady)".ToLowerInvariant(),
      "--receipt-name", (Split-Path -Leaf $pendingReceipt),
      "--deadline-ms", "$DeadlineMs"
    )
  }
  Invoke-ControlledNodeCollector $node $arguments $JourneyKind
  if (-not (Test-Path -LiteralPath $pendingReceipt -PathType Leaf)) { throw "Native IDE journey did not retain its pending machine receipt." }
} catch {
  $journeyFailure = $_
} finally {
  [Environment]::SetEnvironmentVariable("SAGE_DISPOSABLE_PROFILE_ROOT", $previous.profile, "Process")
  [Environment]::SetEnvironmentVariable("SAGE_PROJECT_ROOT", $previous.project, "Process")
  [Environment]::SetEnvironmentVariable("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", $previous.webview, "Process")
  try { $sageCleanup = Stop-OwnedRuntimeTree "sage" $sageRuntime $EvidenceRoot } catch { $cleanupFailures += $_.Exception.Message }
  try { $viteCleanup = Stop-OwnedRuntimeTree "vite" $viteRuntime $EvidenceRoot } catch { $cleanupFailures += $_.Exception.Message }
}

if ($cleanupFailures.Count -ne 0) { throw ($cleanupFailures -join " ") }
if ($null -ne $journeyFailure) { throw $journeyFailure }
if ($JourneyKind -eq "acp04-live" -or $JourneyKind -eq "av01-preview") {
  $nativeStdoutIdentity = Assert-BoundedNativeLog $sageStdoutLog "Sage stdout"
  $nativeStderrIdentity = Assert-BoundedNativeLog $sageStderrLog "Sage stderr"
}
$receipt = Join-Path $EvidenceRoot "machine-receipt.json"
if (-not (Test-Path -LiteralPath $pendingReceipt -PathType Leaf)) { throw "Native IDE journey cleanup completed without a pending receipt." }
if ($JourneyKind -eq "acp04-live" -or $JourneyKind -eq "av01-preview") {
  $sageCleanupJson = $sageCleanup | ConvertTo-Json -Compress
  $viteCleanupJson = $viteCleanup | ConvertTo-Json -Compress
  $finalizerArguments = @(
    $finalizer,
    "--pending", $pendingReceipt,
    "--final", $receipt,
    "--source-root", $resolvedSource,
    "--stdout", $sageStdoutLog,
    "--stderr", $sageStderrLog,
    "--sage-cleanup", [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($sageCleanupJson)),
    "--vite-cleanup", [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($viteCleanupJson)),
    "--deadline-ms", "30000"
  )
  Invoke-ControlledNodeCollector $node $finalizerArguments $JourneyKind
  if ((Test-Path -LiteralPath $pendingReceipt) -or
      -not (Test-Path -LiteralPath $receipt -PathType Leaf)) {
    throw "Source-functional final receipt publication did not complete."
  }
} else {
  Publish-PendingJourneyReceipt $pendingReceipt $receipt $sageCleanup $viteCleanup
}
Write-Output $receipt
