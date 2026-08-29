[CmdletBinding()]
param(
  [string]$SageExecutable,
  [string]$SourceRoot,
  [string]$EvidenceRoot,
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
  @'
export const SAGE_JOURNEY_SENTINEL: number = "diagnostic";
'@ | Set-Content -LiteralPath (Join-Path $Fixture "journey.ts") -NoNewline -Encoding utf8
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

function New-OwnedRuntime([System.Diagnostics.Process]$Process, [string]$ExecutablePath) {
  $identity = Get-FootprintLaunchIdentity $Process $ExecutablePath
  $store = New-FootprintIdentityStore
  [void](Add-FootprintRememberedIdentity $store ([pscustomobject]@{
    ProcessId = $identity.processId
    ExecutablePath = $identity.executablePath
    CreationDate = $identity.creationDate
  }))
  return [ordered]@{ process = $Process; identity = $identity; store = $store }
}

function Write-LaunchIdentityQuarantine(
  [ValidateSet("sage", "vite")][string]$Role,
  [string]$EvidenceRoot,
  [int]$ObservedPid
) {
  if (-not $EvidenceRoot -or -not (Test-Path -LiteralPath $EvidenceRoot -PathType Container)) { return }
  [ordered]@{
    schemaVersion = 1
    runtimeRole = $Role
    category = "identity-capture-failed"
    observedPid = $ObservedPid
  } | ConvertTo-Json -Compress | Set-Content `
      -LiteralPath (Join-Path $EvidenceRoot ("runtime-launch-quarantine-{0}.json" -f $Role)) `
      -NoNewline -Encoding utf8
}

function Capture-OwnedRuntimeOrQuarantine(
  [System.Diagnostics.Process]$Process,
  [string]$ExecutablePath,
  [ValidateSet("sage", "vite")][string]$Role,
  [string]$EvidenceRoot
) {
  try {
    return New-OwnedRuntime $Process $ExecutablePath
  } catch {
    Write-LaunchIdentityQuarantine $Role $EvidenceRoot ([int]$Process.Id)
    throw "The $Role runtime identity could not be captured."
  }
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
  [string]$EvidenceRoot
) {
  Assert-FrontendPortAvailable $Contract
  $npm = @(Get-Command npm.cmd -CommandType Application -ErrorAction Stop) | Select-Object -First 1
  $command = @(Get-Command cmd.exe -CommandType Application -ErrorAction Stop) | Select-Object -First 1
  $npmPath = Require-AbsoluteFile $npm.Source "npm launcher"
  $commandPath = Require-AbsoluteFile $command.Source "command interpreter"
  $commandLine = '/d /s /c ""{0}" run dev -- --host 127.0.0.1 --port {1}"' -f $npmPath, $Contract.port
  $process = Start-Process -FilePath $commandPath -ArgumentList $commandLine -WorkingDirectory $SourceRoot -WindowStyle Hidden -PassThru
  return Capture-OwnedRuntimeOrQuarantine $process $commandPath "vite" $EvidenceRoot
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
if (-not (Test-Path -LiteralPath (Join-Path $resolvedSource ".git") -PathType Container)) { throw "SourceRoot must be a Git checkout." }
$node = Require-AbsoluteFile (Get-Command node -ErrorAction Stop).Source "node"
$runner = Require-AbsoluteFile (Join-Path $PSScriptRoot "source-functional-native-ide-journey.mjs") "journey runner"
$playwrightCli = Require-AbsoluteFile $env:SAGE_PLAYWRIGHT_CLI "SAGE_PLAYWRIGHT_CLI"
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
  $EvidenceRoot = Join-Path $resolvedSource "local-evidence\BETA-0001\source-functional\native-ide-$stamp"
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
$profileIdentity = New-C0BDisposableProfile $EvidenceRoot "native-ide-profile"
$profile = $profileIdentity.root

$fixtureIdentity = Initialize-JourneyFixture $fixture
$fixtureIdentity |
  ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $fixture "fixture-identity.json") -NoNewline -Encoding utf8

$port = Get-BoundedLoopbackPort
$previous = @{
  profile = [Environment]::GetEnvironmentVariable("SAGE_DISPOSABLE_PROFILE_ROOT", "Process")
  project = [Environment]::GetEnvironmentVariable("SAGE_PROJECT_ROOT", "Process")
  webview = [Environment]::GetEnvironmentVariable("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "Process")
}
$startupDeadline = [DateTime]::UtcNow.AddMilliseconds([Math]::Min(30000, $DeadlineMs))
$viteRuntime = $null
$sageRuntime = $null
$runtimeReceipt = $null
$journeyFailure = $null
$cleanupFailures = @()
$pendingReceipt = Join-Path $EvidenceRoot "machine-receipt.pending.json"
$sageCleanup = $null
$viteCleanup = $null
try {
  $viteRuntime = Start-OwnedVite $resolvedSource $frontendContract $EvidenceRoot
  $runtimeReceipt = Wait-OwnedFrontendReady $viteRuntime $frontendContract $startupDeadline
  [Environment]::SetEnvironmentVariable("SAGE_DISPOSABLE_PROFILE_ROOT", $profile, "Process")
  [Environment]::SetEnvironmentVariable("SAGE_PROJECT_ROOT", $fixture, "Process")
  [Environment]::SetEnvironmentVariable("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--remote-debugging-port=$port", "Process")
  $process = Start-Process -FilePath $resolvedExecutable -WorkingDirectory (Split-Path -Parent $resolvedExecutable) -PassThru
  $sageRuntime = Capture-OwnedRuntimeOrQuarantine $process $resolvedExecutable "sage" $EvidenceRoot
  $arguments = @(
    $runner, "--port", "$port", "--output", $EvidenceRoot,
    "--source-commit", $commit, "--source-tree", $tree,
    "--executable", $resolvedExecutable, "--fixture", $fixture,
    "--source-root", $resolvedSource, "--playwright-cli", $playwrightCli,
    "--executable-bytes", "$preLaunchExecutableBytes", "--executable-sha256", $preLaunchExecutableSha256,
    "--expected-origin", $frontendContract.origin,
    "--runtime-mode", $runtimeReceipt.mode, "--frontend-ready", "$($runtimeReceipt.frontendReady)".ToLowerInvariant(),
    "--receipt-name", (Split-Path -Leaf $pendingReceipt),
    "--deadline-ms", "$DeadlineMs"
  )
  & $node @arguments
  if ($LASTEXITCODE -ne 0) { throw "Native IDE journey runner failed; inspect machine-failure.json under EvidenceRoot." }
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
$receipt = Join-Path $EvidenceRoot "machine-receipt.json"
if (-not (Test-Path -LiteralPath $pendingReceipt -PathType Leaf)) { throw "Native IDE journey cleanup completed without a pending receipt." }
Publish-PendingJourneyReceipt $pendingReceipt $receipt $sageCleanup $viteCleanup
Write-Output $receipt
