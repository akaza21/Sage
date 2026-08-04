Set-StrictMode -Version Latest

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$script:DebugEnvironmentNames = @(
  "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
  "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER"
)
$script:MaxBindingBytes = 1MB
$script:MaxManifestBytes = 8MB
$script:MaxManifestFiles = 4096
$script:MaxManifestSourceBytes = 512MB
$script:MaxDiagnosticStateFileBytes = 512MB
$script:MaxNormalDataEntries = 4096
$script:MaxNormalDataBytes = 512MB
$script:MaxNormalDataPathBytes = 4MB
$script:DiagnosticDataEnvironmentName = "SAGE_DIAGNOSTIC_DATA_DIR"
$script:FixtureProjectEnvironmentName = "SAGE_PROJECT_ROOT"
$script:MaxFixtureEntries = 64
$script:MaxFixtureFiles = 32
$script:MaxFixtureBytes = 1MB

function Read-FootprintBoundedJsonFile {
  param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory)][long]$MaxBytes,
    [Parameter(Mandatory)][string]$Label
  )
  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "$Label is missing." }
  $resolved = (Resolve-Path -LiteralPath $Path).Path
  $stream = [IO.File]::Open($resolved, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
  try {
    if ($stream.Length -gt $MaxBytes) { throw "$Label exceeds its $MaxBytes-byte limit." }
    $bytes = [byte[]]::new([int]$stream.Length)
    $offset = 0
    while ($offset -lt $bytes.Length) {
      $read = $stream.Read($bytes, $offset, $bytes.Length - $offset)
      if ($read -eq 0) { throw "$Label changed while it was being read." }
      $offset += $read
    }
    if ($stream.ReadByte() -ne -1) { throw "$Label changed while it was being read." }
  } finally {
    $stream.Dispose()
  }
  $sha = [Security.Cryptography.SHA256]::Create()
  try {
    $hash = ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace("-", "").ToLowerInvariant()
  } finally {
    $sha.Dispose()
  }
  $decoder = [Text.UTF8Encoding]::new($false, $true)
  $text = $decoder.GetString($bytes)
  if ($text.Length -gt 0 -and $text[0] -eq [char]0xFEFF) { $text = $text.Substring(1) }
  return [ordered]@{
    json = $text | ConvertFrom-Json
    path = $resolved
    sha256 = $hash
  }
}

function Invoke-FootprintBoundedProcess {
  param(
    [Parameter(Mandatory)][string]$WrapperNodePath,
    [Parameter(Mandatory)][string]$RunnerPath,
    [Parameter(Mandatory)][string]$ExecutablePath,
    [Parameter(Mandatory)][string[]]$Arguments,
    [Parameter(Mandatory)][string]$ResultPath,
    [ValidateRange(100, 120000)][int]$TimeoutMs = 15000,
    [ValidateRange(1, 1048576)][int]$MaxOutputBytes = 65536
  )
  foreach ($candidate in @($WrapperNodePath, $RunnerPath, $ExecutablePath)) {
    if (-not [IO.Path]::IsPathRooted($candidate) -or -not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
      throw "Bounded Node execution requires existing absolute executable and runner paths."
    }
  }
  if (-not [IO.Path]::IsPathRooted($ResultPath) -or (Test-Path -LiteralPath $ResultPath)) {
    throw "Bounded Node execution requires a fresh absolute result path."
  }
  if ($Arguments.Count -gt 32) { throw "Bounded Node execution allows at most 32 arguments." }
  foreach ($argument in $Arguments) {
    if ([Text.Encoding]::UTF8.GetByteCount($argument) -gt 4096) { throw "Bounded Node argument exceeds 4,096 bytes." }
  }
  $resolvedWrapperNode = (Resolve-Path -LiteralPath $WrapperNodePath).Path
  $resolvedRunner = (Resolve-Path -LiteralPath $RunnerPath).Path
  $resolvedExecutable = (Resolve-Path -LiteralPath $ExecutablePath).Path
  $payload = [ordered]@{
    executablePath = $resolvedExecutable
    arguments = $Arguments
    timeoutMs = $TimeoutMs
    maxOutputBytes = $MaxOutputBytes
    resultPath = [IO.Path]::GetFullPath($ResultPath)
  } | ConvertTo-Json -Compress
  $encoded = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($payload))
  $wrapper = Start-Process -FilePath $resolvedWrapperNode -ArgumentList @("`"$resolvedRunner`"", $encoded) -PassThru -WindowStyle Hidden
  if (-not $wrapper.WaitForExit($TimeoutMs + 5000)) {
    try { $wrapper.Kill() } catch {}
    [void]$wrapper.WaitForExit(5000)
    throw "Bounded Node runner exceeded its outer timeout."
  }
  if ($wrapper.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $ResultPath -PathType Leaf)) {
    throw "Bounded Node runner failed with exit code $($wrapper.ExitCode)."
  }
  $record = Read-FootprintBoundedJsonFile $ResultPath ([long]$MaxOutputBytes * 2 + 65536) "Bounded Node result"
  $result = $record.json
  if ([int]$result.schemaVersion -ne 1 -or $result.executablePath -ne $resolvedExecutable) {
    throw "Bounded Node result identity is invalid."
  }
  if (-not [string]::IsNullOrWhiteSpace([string]$result.terminalReason)) {
    throw "Bounded Node process failed: $($result.terminalReason)."
  }
  if (-not [string]::IsNullOrWhiteSpace([string]$result.spawnError)) {
    throw "Bounded Node process could not start: $($result.spawnError)"
  }
  if ($null -eq $result.exitCode -or [int]$result.exitCode -ne 0) {
    throw "Bounded Node process exited with code $($result.exitCode). stderr: $($result.stderr)"
  }
  return [ordered]@{
    exitCode = [int]$result.exitCode
    stdout = [string]$result.stdout
    stderr = [string]$result.stderr
    stdoutBytes = [int]$result.stdoutBytes
    stderrBytes = [int]$result.stderrBytes
    resultSha256 = $record.sha256
  }
}

function Get-FootprintNormalizedPath {
  param([Parameter(Mandatory)][string]$Path)
  return [IO.Path]::GetFullPath($Path).TrimEnd([char[]]@('\', '/'))
}

function Test-FootprintPathWithin {
  param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory)][string]$Parent
  )
  $pathKey = Get-FootprintNormalizedPath $Path
  $parentKey = Get-FootprintNormalizedPath $Parent
  if ([string]::Equals($pathKey, $parentKey, [StringComparison]::OrdinalIgnoreCase)) {
    return $true
  }
  $prefix = $parentKey + [IO.Path]::DirectorySeparatorChar
  return $pathKey.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)
}

function Get-FootprintStringSha256 {
  param([Parameter(Mandatory)][string]$Value)
  $sha = [Security.Cryptography.SHA256]::Create()
  try {
    $bytes = [Text.Encoding]::UTF8.GetBytes($Value)
    return ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace("-", "").ToLowerInvariant()
  } finally {
    $sha.Dispose()
  }
}

function Assert-FootprintDiagnosticEnvironmentClear {
  $inherited = [Environment]::GetEnvironmentVariable($script:DiagnosticDataEnvironmentName, "Process")
  if (-not [string]::IsNullOrWhiteSpace($inherited)) {
    throw "Footprint measurement refuses an inherited $($script:DiagnosticDataEnvironmentName) value."
  }
  return $true
}

function Assert-FootprintFixtureEnvironmentClear {
  $inherited = [Environment]::GetEnvironmentVariable($script:FixtureProjectEnvironmentName, "Process")
  if (-not [string]::IsNullOrWhiteSpace($inherited)) {
    throw "Footprint measurement refuses an inherited $($script:FixtureProjectEnvironmentName) value."
  }
  return $true
}

function Assert-FootprintDiagnosticCampaignInputs {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$Binding,
    [AllowEmptyString()][string]$DiagnosticDataDirectory
  )
  if (-not [bool]$Binding.diagnosticsFeature) {
    if (-not [string]::IsNullOrWhiteSpace($DiagnosticDataDirectory)) {
      throw "A production candidate forbids SAGE_DIAGNOSTIC_DATA_DIR because it does not honor that override."
    }
    throw "Production footprint measurement requires the separately approved disposable-profile harness."
  }
  if ([string]::IsNullOrWhiteSpace($DiagnosticDataDirectory)) {
    throw "A diagnostics candidate requires an explicit isolated DiagnosticDataDirectory."
  }
  return $true
}

function Get-FootprintDiagnosticDataProvenance {
  param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory)][string]$WorkspaceRoot,
    [Parameter(Mandatory)][string]$OutputDirectory
  )
  if (-not [IO.Path]::IsPathRooted($Path) -or -not (Test-Path -LiteralPath $Path -PathType Container)) {
    throw "DiagnosticDataDirectory must identify an existing absolute directory."
  }
  $resolved = (Resolve-Path -LiteralPath $Path).Path
  $resolvedWorkspace = (Resolve-Path -LiteralPath $WorkspaceRoot).Path
  $localEvidence = Join-Path $resolvedWorkspace "local-evidence"
  if ([string]::Equals((Get-FootprintNormalizedPath $resolved), (Get-FootprintNormalizedPath $localEvidence), [StringComparison]::OrdinalIgnoreCase) -or
      -not (Test-FootprintPathWithin $resolved $localEvidence)) {
    throw "DiagnosticDataDirectory must be a child of the workspace's ignored local-evidence directory."
  }
  $cursor = Get-Item -LiteralPath $resolved
  while ($true) {
    if (($cursor.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
      throw "DiagnosticDataDirectory cannot traverse a reparse point."
    }
    if ([string]::Equals((Get-FootprintNormalizedPath $cursor.FullName), (Get-FootprintNormalizedPath $localEvidence), [StringComparison]::OrdinalIgnoreCase)) {
      break
    }
    $cursor = $cursor.Parent
    if ($null -eq $cursor) { throw "DiagnosticDataDirectory containment could not be proven." }
  }
  $normalizedOutput = Get-FootprintNormalizedPath $OutputDirectory
  if ((Test-FootprintPathWithin $resolved $normalizedOutput) -or
      (Test-FootprintPathWithin $normalizedOutput $resolved)) {
    throw "DiagnosticDataDirectory and OutputDirectory must be disjoint."
  }
  $normalDataRoots = @(
    [Environment]::GetFolderPath([Environment+SpecialFolder]::ApplicationData),
    [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
  ) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | ForEach-Object {
    Join-Path $_ "ai.sage.desktop"
  }
  foreach ($normalData in $normalDataRoots) {
    if ((Test-FootprintPathWithin $resolved $normalData) -or
        (Test-FootprintPathWithin $normalData $resolved)) {
      throw "DiagnosticDataDirectory overlaps Sage's normal application-data boundary."
    }
  }
  $normalized = Get-FootprintNormalizedPath $resolved
  if ([Text.Encoding]::UTF8.GetByteCount($normalized) -gt 4096) {
    throw "DiagnosticDataDirectory exceeds its 4,096-byte path limit."
  }
  $diagnosticEntries = @(Get-ChildItem -LiteralPath $resolved -Force | Select-Object -First 33)
  if ($diagnosticEntries.Count -gt 32) {
    throw "DiagnosticDataDirectory exceeds its 32-entry limit."
  }
  $stateFiles = @()
  foreach ($name in @("sage.db", "sage.db-wal", "sage.db-shm")) {
    $statePath = Join-Path $resolved $name
    if (Test-Path -LiteralPath $statePath -PathType Leaf) {
      $stateItem = Get-Item -LiteralPath $statePath
      if (($stateItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "Diagnostic state file '$name' cannot be a reparse point."
      }
      if ($stateItem.Length -gt $script:MaxDiagnosticStateFileBytes) {
        throw "Diagnostic state file '$name' exceeds its $($script:MaxDiagnosticStateFileBytes)-byte limit."
      }
      $stateFiles += [ordered]@{
        name = $name
        exists = $true
        bytes = [long]$stateItem.Length
        sha256 = (Get-FileHash -LiteralPath $statePath -Algorithm SHA256).Hash.ToLowerInvariant()
      }
    } else {
      $stateFiles += [ordered]@{ name = $name; exists = $false; bytes = 0; sha256 = $null }
    }
  }
  return [ordered]@{
    schemaVersion = 1
    purpose = "isolated-diagnostics"
    environmentVariable = $script:DiagnosticDataEnvironmentName
    path = $normalized
    pathSha256 = Get-FootprintStringSha256 ($normalized.ToLowerInvariant())
    withinIgnoredLocalEvidence = $true
    reparsePoint = $false
    entryCount = $diagnosticEntries.Count
    stateFiles = $stateFiles
  }
}

function Get-FootprintFixtureProjectProvenance {
  param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory)][string]$WorkspaceRoot,
    [Parameter(Mandatory)][string]$DiagnosticDataDirectory,
    [Parameter(Mandatory)][string]$OutputDirectory
  )
  if (-not [IO.Path]::IsPathRooted($Path) -or -not (Test-Path -LiteralPath $Path -PathType Container)) {
    throw "FixtureProjectRoot must identify an existing absolute directory."
  }
  $resolved = (Resolve-Path -LiteralPath $Path).Path
  $localEvidence = Join-Path (Resolve-Path -LiteralPath $WorkspaceRoot).Path "local-evidence"
  if ([string]::Equals((Get-FootprintNormalizedPath $resolved), (Get-FootprintNormalizedPath $localEvidence), [StringComparison]::OrdinalIgnoreCase) -or
      -not (Test-FootprintPathWithin $resolved $localEvidence)) {
    throw "FixtureProjectRoot must be a child of the workspace's ignored local-evidence directory."
  }
  foreach ($boundary in @($DiagnosticDataDirectory, $OutputDirectory)) {
    if ((Test-FootprintPathWithin $resolved $boundary) -or (Test-FootprintPathWithin $boundary $resolved)) {
      throw "FixtureProjectRoot must be disjoint from diagnostics data and campaign output."
    }
  }
  $cursor = Get-Item -LiteralPath $resolved
  while ($true) {
    if (($cursor.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw "FixtureProjectRoot cannot traverse a reparse point." }
    if ([string]::Equals((Get-FootprintNormalizedPath $cursor.FullName), (Get-FootprintNormalizedPath $localEvidence), [StringComparison]::OrdinalIgnoreCase)) { break }
    $cursor = $cursor.Parent
    if ($null -eq $cursor) { throw "FixtureProjectRoot containment could not be proven." }
  }
  $items = @(Get-ChildItem -LiteralPath $resolved -Force -Recurse | Select-Object -First ($script:MaxFixtureEntries + 1))
  if ($items.Count -gt $script:MaxFixtureEntries) { throw "FixtureProjectRoot exceeds its $($script:MaxFixtureEntries)-entry limit." }
  if (@($items | Where-Object Name -eq ".git").Count -ne 0) { throw "FixtureProjectRoot must be an ordinary non-repository folder." }
  $files = @($items | Where-Object { -not $_.PSIsContainer })
  if ($files.Count -lt 1 -or $files.Count -gt $script:MaxFixtureFiles) { throw "FixtureProjectRoot must contain 1..$($script:MaxFixtureFiles) seed files." }
  $totalBytes = [long]0
  $records = @()
  foreach ($item in $items) {
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw "FixtureProjectRoot cannot contain reparse points." }
    $relative = $item.FullName.Substring($resolved.Length).TrimStart([char[]]@('\', '/')).Replace('\', '/')
    if ($item.PSIsContainer) { $records += "D`0$relative"; continue }
    $totalBytes += [long]$item.Length
    if ($totalBytes -gt $script:MaxFixtureBytes) { throw "FixtureProjectRoot exceeds its $($script:MaxFixtureBytes)-byte seed limit." }
    $hash = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    $records += "F`0$relative`0$($item.Length)`0$hash"
  }
  return [ordered]@{
    schemaVersion = 1
    purpose = "native-project-seed"
    environmentVariable = $script:FixtureProjectEnvironmentName
    path = Get-FootprintNormalizedPath $resolved
    pathSha256 = Get-FootprintStringSha256 ((Get-FootprintNormalizedPath $resolved).ToLowerInvariant())
    entryCount = $items.Count
    fileCount = $files.Count
    totalBytes = $totalBytes
    aggregateSha256 = Get-FootprintStringSha256 (@($records | Sort-Object) -join "`n")
    ordinaryFolder = $true
  }
}

function Assert-FootprintFreshDiagnosticData {
  param([Parameter(Mandatory)][Collections.IDictionary]$Provenance)
  if ([int]$Provenance.entryCount -ne 0 -or @($Provenance.stateFiles | Where-Object exists).Count -ne 0) {
    throw "Diagnostics measurement requires a fresh empty scenario-specific data directory."
  }
  return $true
}

function Assert-FootprintDiagnosticStateChanged {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$Before,
    [Parameter(Mandatory)][Collections.IDictionary]$After
  )
  $beforeDb = @($Before.stateFiles | Where-Object name -eq "sage.db")
  $afterDb = @($After.stateFiles | Where-Object name -eq "sage.db")
  if ($beforeDb.Count -ne 1 -or $afterDb.Count -ne 1 -or $beforeDb[0].exists -or
      -not $afterDb[0].exists -or [long]$afterDb[0].bytes -le 0 -or
      [string]::IsNullOrWhiteSpace([string]$afterDb[0].sha256)) {
    throw "The exact launch did not create isolated diagnostic database state."
  }
  return $true
}

function Get-FootprintBoundedDirectoryInventory {
  param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory)][string]$Label
  )
  $pathHash = Get-FootprintStringSha256 ((Get-FootprintNormalizedPath $Path).ToLowerInvariant())
  if (-not (Test-Path -LiteralPath $Path)) {
    return [ordered]@{ label = $Label; pathSha256 = $pathHash; exists = $false; entryCount = 0; totalFileBytes = 0; aggregateSha256 = Get-FootprintStringSha256 "absent" }
  }
  if (-not (Test-Path -LiteralPath $Path -PathType Container)) { throw "$Label is not a directory." }
  $root = (Resolve-Path -LiteralPath $Path).Path
  $items = @(Get-ChildItem -LiteralPath $root -Force -Recurse | Select-Object -First ($script:MaxNormalDataEntries + 1))
  if ($items.Count -gt $script:MaxNormalDataEntries) {
    throw "$Label exceeds its $($script:MaxNormalDataEntries)-entry inventory limit."
  }
  $records = @()
  $totalBytes = [long]0
  $pathBytes = [long]0
  foreach ($item in $items) {
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw "$Label contains a reparse point." }
    $relative = $item.FullName.Substring($root.Length).TrimStart([char[]]@('\', '/')).Replace('\', '/')
    $pathBytes += [Text.Encoding]::UTF8.GetByteCount($relative)
    if ($pathBytes -gt $script:MaxNormalDataPathBytes) { throw "$Label exceeds its retained path-byte limit." }
    if ($item.PSIsContainer) {
      $records += "D`0$relative"
      continue
    }
    $lengthBefore = [long]$item.Length
    $totalBytes += $lengthBefore
    if ($totalBytes -gt $script:MaxNormalDataBytes) { throw "$Label exceeds its $($script:MaxNormalDataBytes)-byte inventory limit." }
    $hash = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    $lengthAfter = [long](Get-Item -LiteralPath $item.FullName).Length
    if ($lengthBefore -ne $lengthAfter) { throw "$Label changed while it was inventoried." }
    $records += "F`0$relative`0$lengthBefore`0$hash"
  }
  $orderedRecords = @($records | Sort-Object)
  return [ordered]@{
    label = $Label
    pathSha256 = $pathHash
    exists = $true
    entryCount = $items.Count
    totalFileBytes = $totalBytes
    aggregateSha256 = Get-FootprintStringSha256 ($orderedRecords -join "`n")
  }
}

function Get-FootprintNormalDataInventory {
  $roamingPath = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::ApplicationData)) "ai.sage.desktop"
  $localPath = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)) "ai.sage.desktop"
  $durable = Get-FootprintBoundedDirectoryInventory $roamingPath "roaming-durable-ai.sage.desktop"
  return [ordered]@{
    schemaVersion = 2
    durableRoaming = $durable
    durableAggregateSha256 = $durable.aggregateSha256
    disposableLocalCache = [ordered]@{
      label = "local-webview-cache-ai.sage.desktop"
      pathSha256 = Get-FootprintStringSha256 ((Get-FootprintNormalizedPath $localPath).ToLowerInvariant())
      existsAtCapture = Test-Path -LiteralPath $localPath -PathType Container
      policy = "disposable-uncontrolled-not-an-isolation-gate"
      expectedChurn = "EBWebView and Tauri runtime cache may change during an isolated SQLite campaign"
    }
  }
}

function Assert-FootprintNormalDataUnchanged {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$Before,
    [Parameter(Mandatory)][Collections.IDictionary]$After
  )
  if ($Before.durableAggregateSha256 -ne $After.durableAggregateSha256) {
    throw "Normal durable Roaming Sage state changed during an isolated diagnostics launch."
  }
  return $true
}

function Invoke-FootprintNormalDataGuardedAction {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$Before,
    [Parameter(Mandatory)][scriptblock]$Action,
    [Parameter(Mandatory)][scriptblock]$CaptureAfter
  )
  $result = $null
  $after = $null
  $actionFailure = $null
  $guardFailure = $null
  try {
    $result = & $Action
  } catch {
    $actionFailure = $_
  } finally {
    try {
      $after = & $CaptureAfter
      [void](Assert-FootprintNormalDataUnchanged $Before $after)
    } catch {
      $guardFailure = $_
    }
  }
  if ($null -ne $guardFailure) {
    if ($null -ne $actionFailure) {
      throw "Setup failed: $($actionFailure.Exception.Message) Normal-data isolation guard also failed: $($guardFailure.Exception.Message)"
    }
    throw $guardFailure
  }
  if ($null -ne $actionFailure) { throw $actionFailure }
  return [ordered]@{ result = $result; after = $after }
}

function Get-FootprintScenarioContract {
  param([Parameter(Mandatory)][ValidateSet("chat", "explorer")][string]$Scenario)

  return [ordered]@{
    scenario = $Scenario
    setupRequired = $true
    setupVerificationRequired = $true
    measuredVerificationRequired = $true
    chatCenterRequired = $true
    explorerSidebarRequired = $Scenario -eq "explorer"
    explorerSidebarCount = if ($Scenario -eq "explorer") { 1 } else { 0 }
    explorerHeavyViewMounted = $Scenario -eq "explorer"
    exactTitle = "Sage"
    exactOrigin = "http://tauri.localhost/"
  }
}

function Get-FootprintChatTransitionDecision {
  param(
    [Parameter(Mandatory)][int]$ChatCenterCount,
    [Parameter(Mandatory)][int]$ExplorerSidebarCount,
    [Parameter(Mandatory)][int]$HideSidebarButtonCount,
    [Parameter(Mandatory)][int]$ShowSidebarButtonCount
  )
  if ($ChatCenterCount -eq 1 -and $ExplorerSidebarCount -eq 1 -and $HideSidebarButtonCount -eq 1 -and $ShowSidebarButtonCount -eq 0) {
    return [ordered]@{ action = "invoke-hide-right-sidebar"; invokeCount = 1 }
  }
  if ($ChatCenterCount -eq 1 -and $ExplorerSidebarCount -eq 0 -and $HideSidebarButtonCount -eq 0 -and $ShowSidebarButtonCount -eq 1) {
    return [ordered]@{ action = "already-closed-noop"; invokeCount = 0 }
  }
  if ($ChatCenterCount -eq 1 -and $ExplorerSidebarCount -eq 0 -and $HideSidebarButtonCount -eq 0 -and $ShowSidebarButtonCount -eq 0) {
    return [ordered]@{ action = "already-closed-provider-unexposed"; invokeCount = 0 }
  }
  throw "Measured Chat launch did not expose one exact open or already-closed sidebar state."
}

function Wait-FootprintChatTransitionDecision {
  param(
    [Parameter(Mandatory)][scriptblock]$SnapshotProvider,
    [ValidateRange(1, 301)][int]$MaxAttempts,
    [ValidateRange(0, 1000)][int]$PollMilliseconds = 100
  )
  $last = [ordered]@{ uiaView = "unknown"; chatCenterCount = -1; explorerSidebarCount = -1; hideSidebarButtonCount = -1; showSidebarButtonCount = -1; visitedElementCount = 0 }
  $lastProviderError = $null
  for ($attempt = 1; $attempt -le $MaxAttempts; $attempt += 1) {
    try {
      $snapshot = & $SnapshotProvider
      foreach ($field in @("uiaView", "chatCenterCount", "explorerSidebarCount", "hideSidebarButtonCount", "showSidebarButtonCount", "visitedElementCount")) {
        if (-not $snapshot.Contains($field)) { throw "Chat transition snapshot is missing '$field'." }
      }
      if ($snapshot.uiaView -notin @("control", "raw")) { throw "Chat transition snapshot has an unsupported UIA view." }
    } catch {
      $lastProviderError = ([string]$_.Exception.Message -replace "[`r`n]+", " ").Trim()
      if ($lastProviderError.Length -gt 512) { $lastProviderError = $lastProviderError.Substring(0, 512) }
      if ($attempt -lt $MaxAttempts -and $PollMilliseconds -gt 0) { Start-Sleep -Milliseconds $PollMilliseconds }
      continue
    }
    $lastProviderError = $null
    $last = $snapshot
    try {
      $decision = Get-FootprintChatTransitionDecision `
        ([int]$snapshot.chatCenterCount) `
        ([int]$snapshot.explorerSidebarCount) `
        ([int]$snapshot.hideSidebarButtonCount) `
        ([int]$snapshot.showSidebarButtonCount)
      return [ordered]@{ decision = $decision; snapshot = $snapshot; attempts = $attempt }
    } catch {
      if ($attempt -lt $MaxAttempts -and $PollMilliseconds -gt 0) {
        Start-Sleep -Milliseconds $PollMilliseconds
      }
    }
  }
  $providerDiagnostic = if ($null -eq $lastProviderError) { "none" } else { $lastProviderError }
  throw "Chat transition readiness timed out; final view=$($last.uiaView), chat=$($last.chatCenterCount), explorer=$($last.explorerSidebarCount), hide=$($last.hideSidebarButtonCount), show=$($last.showSidebarButtonCount), visited=$($last.visitedElementCount), traversalCap=4096, providerError=$providerDiagnostic."
}

function Select-FootprintUiaViewSnapshot {
  param(
    [Parameter(Mandatory)][Collections.IDictionary]$ControlSnapshot,
    [Parameter(Mandatory)][scriptblock]$RawSnapshotProvider
  )
  if ($ControlSnapshot.uiaView -ne "control") { throw "UIA view selection requires a ControlView snapshot first." }
  if ([int]$ControlSnapshot.chatCenterCount -eq 0) {
    $raw = & $RawSnapshotProvider
    if ($raw.uiaView -ne "raw") { throw "UIA fallback did not return an independent RawView snapshot." }
    return $raw
  }
  return $ControlSnapshot
}

function Get-FootprintUiaViewSnapshot {
  param(
    [Parameter(Mandatory)][IntPtr]$WindowHandle,
    [Parameter(Mandatory)][int]$ExpectedProcessId,
    [Parameter(Mandatory)][ValidateSet("control", "raw")][string]$UiaView
  )
  $root = [Windows.Automation.AutomationElement]::FromHandle($WindowHandle)
  if ($null -eq $root -or [int]$root.Current.ProcessId -ne $ExpectedProcessId -or $root.Current.Name -ne "Sage") {
    throw "$UiaView UIA root does not identify the exact measured Sage process."
  }
  $walker = if ($UiaView -eq "control") {
    [Windows.Automation.TreeWalker]::ControlViewWalker
  } else {
    [Windows.Automation.TreeWalker]::RawViewWalker
  }
  $current = $walker.GetFirstChild($root)
  $visited = 0
  $named = 0
  $chatCount = 0
  $explorerCount = 0
  $hideButtons = @()
  $showCount = 0
  while ($null -ne $current) {
    $visited += 1
    if ($visited -gt 4096) { throw "$UiaView UI Automation traversal exceeded 4,096 elements." }
    if ([int]$current.Current.ProcessId -eq $ExpectedProcessId) {
      $name = [string]$current.Current.Name
      if (-not [string]::IsNullOrWhiteSpace($name)) { $named += 1 }
      if ($name.Trim() -eq "Agent chat") { $chatCount += 1 }
      if ($name.Trim() -eq "Project files") { $explorerCount += 1 }
      if ($current.Current.ControlType -eq [Windows.Automation.ControlType]::Button) {
        if ($name.Trim() -eq "Hide right sidebar") { $hideButtons += $current }
        if ($name.Trim() -eq "Show right sidebar") { $showCount += 1 }
      }
    }
    $child = $walker.GetFirstChild($current)
    if ($null -ne $child) { $current = $child; continue }
    while ($null -ne $current) {
      $sibling = $walker.GetNextSibling($current)
      if ($null -ne $sibling) { $current = $sibling; break }
      $parent = $walker.GetParent($current)
      if ($null -eq $parent -or $parent.Equals($root)) { $current = $null; break }
      $current = $parent
    }
  }
  return [ordered]@{
    uiaView = $UiaView
    rootTitle = [string]$root.Current.Name
    chatCenterCount = $chatCount
    explorerSidebarCount = $explorerCount
    hideSidebarButtonCount = $hideButtons.Count
    showSidebarButtonCount = $showCount
    visitedElementCount = $visited
    namedElementCount = $named
    hideSidebarButton = if ($hideButtons.Count -eq 1) { $hideButtons[0] } else { $null }
  }
}

function Get-FootprintPreferredUiaSnapshot {
  param(
    [Parameter(Mandatory)][IntPtr]$WindowHandle,
    [Parameter(Mandatory)][int]$ExpectedProcessId
  )
  $control = Get-FootprintUiaViewSnapshot $WindowHandle $ExpectedProcessId "control"
  return Select-FootprintUiaViewSnapshot $control {
    Get-FootprintUiaViewSnapshot $WindowHandle $ExpectedProcessId "raw"
  }
}

function Get-FootprintChatTransitionSnapshot {
  param(
    [Parameter(Mandatory)][IntPtr]$WindowHandle,
    [Parameter(Mandatory)][int]$ExpectedProcessId
  )
  return Get-FootprintPreferredUiaSnapshot $WindowHandle $ExpectedProcessId
}

function Assert-FootprintDebugStateClear {
  param([AllowNull()][object[]]$ProcessRows)

  foreach ($name in $script:DebugEnvironmentNames) {
    if (-not [string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name, "Process"))) {
      throw "Acceptance refuses the inherited $name variable."
    }
  }
  foreach ($row in @($ProcessRows)) {
    if ($row.commandLine -and [string]$row.commandLine -match "--remote-debugging-(?:port|pipe)") {
      throw "A measured descendant retained WebView2 debugging arguments."
    }
  }
}

function Test-FootprintCandidateBinding {
  param(
    [Parameter(Mandatory)][string]$ExecutablePath,
    [Parameter(Mandatory)][string]$ExpectedExecutableSha256,
    [Parameter(Mandatory)][string]$ExpectedSourceAggregateSha256,
    [Parameter(Mandatory)][string]$CandidateBindingPath,
    [Parameter(Mandatory)][string]$SourceManifestPath
  )

  $bindingRecord = Read-FootprintBoundedJsonFile $CandidateBindingPath $script:MaxBindingBytes "Candidate binding file"
  $manifestRecord = Read-FootprintBoundedJsonFile $SourceManifestPath $script:MaxManifestBytes "Retained source manifest"
  $actual = (Get-FileHash -LiteralPath $ExecutablePath -Algorithm SHA256).Hash.ToLowerInvariant()
  $expectedExecutable = $ExpectedExecutableSha256.ToLowerInvariant()
  $expectedSource = $ExpectedSourceAggregateSha256.ToLowerInvariant()
  if ($actual -ne $expectedExecutable) { throw "Candidate executable hash mismatch." }
  $binding = $bindingRecord.json
  $manifest = $manifestRecord.json
  $manifestAggregate = Get-FootprintSourceManifestAggregate $manifest
  if (([string]$binding.candidate.executableSha256).ToLowerInvariant() -ne $expectedExecutable) {
    throw "Binding file does not identify the exact candidate executable."
  }
  if ($binding.candidate.PSObject.Properties.Name -notcontains "diagnosticsFeature") {
    throw "Binding file does not declare whether the candidate honors isolated diagnostics data."
  }
  if ($binding.candidate.diagnosticsFeature -isnot [bool]) {
    throw "Binding file diagnosticsFeature must be a JSON boolean."
  }
  $cargoFeatures = if ($binding.PSObject.Properties.Name -contains "cargoFeatures") { @($binding.cargoFeatures) } else { @() }
  if ([bool]$binding.candidate.diagnosticsFeature) {
    if ($binding.PSObject.Properties.Name -notcontains "cargoProfile" -or
        $binding.cargoProfile -isnot [string] -or $binding.cargoProfile -cne "release") {
      throw "Diagnostics footprint binding must declare the release Cargo profile."
    }
    if ($cargoFeatures.Count -lt 2 -or $cargoFeatures.Count -gt 16 -or
        @($cargoFeatures | Where-Object { $_ -isnot [string] -or $_ -notmatch "^[a-z0-9-]+$" }).Count -ne 0 -or
        @($cargoFeatures | Sort-Object -Unique).Count -ne $cargoFeatures.Count -or
        "custom-protocol" -notin $cargoFeatures -or "diagnostics" -notin $cargoFeatures) {
      throw "Diagnostics footprint binding must declare unique custom-protocol and diagnostics Cargo features."
    }
  }
  if (([string]$binding.candidate.aggregateSha256).ToLowerInvariant() -ne $expectedSource) {
    throw "Binding file does not identify the exact source aggregate."
  }
  if ([int]$binding.candidate.fileCount -ne [int]$manifest.fileCount -or
      [long]$binding.candidate.totalBytes -ne [long]$manifest.totalBytes) {
    throw "Binding file source counts do not match the retained source manifest."
  }
  if (([string]$manifest.aggregateSha256).ToLowerInvariant() -ne $manifestAggregate -or $manifestAggregate -ne $expectedSource) {
    throw "Retained source manifest does not reproduce the expected aggregate."
  }
  return [ordered]@{
    executableSha256 = $expectedExecutable
    sourceAggregateSha256 = $expectedSource
    bindingPath = $bindingRecord.path
    candidateBindingSha256 = $bindingRecord.sha256
    sourceManifestPath = $manifestRecord.path
    sourceManifestSha256 = $manifestRecord.sha256
    fileCount = [int]$manifest.fileCount
    totalBytes = [long]$manifest.totalBytes
    diagnosticsFeature = [bool]$binding.candidate.diagnosticsFeature
    cargoProfile = if ([bool]$binding.candidate.diagnosticsFeature) { "release" } else { $null }
    cargoFeatures = if ([bool]$binding.candidate.diagnosticsFeature) { $cargoFeatures } else { @() }
  }
}

function Get-FootprintSourceManifestAggregate {
  param([Parameter(Mandatory)]$Manifest)

  $files = @($Manifest.files)
  if ($files.Count -lt 1 -or $files.Count -gt $script:MaxManifestFiles) {
    throw "Source manifest must contain 1..$($script:MaxManifestFiles) files."
  }
  if ([int]$Manifest.schemaVersion -ne 1 -or
      [string]$Manifest.aggregateConstruction -ne "SHA-256(path NUL sha256 NUL byteLength LF)") {
    throw "Source manifest does not use the accepted deterministic schema."
  }
  if ([int]$Manifest.fileCount -ne $files.Count) {
    throw "Source manifest file count does not match its entries."
  }
  $stream = [IO.MemoryStream]::new()
  $totalBytes = [long]0
  $previousPath = $null
  try {
    foreach ($file in $files) {
      $path = [string]$file.path
      $hash = ([string]$file.sha256).ToLowerInvariant()
      $bytes = [long]$file.bytes
      if ([string]::IsNullOrWhiteSpace($path) -or $path.Length -gt 1024 -or
          [IO.Path]::IsPathRooted($path) -or $path.Contains("\") -or
          $path.Contains([char]0) -or $path.Contains("`r") -or $path.Contains("`n") -or
          $path.Split('/') -contains "" -or $path.Split('/') -contains "." -or
          $path.Split('/') -contains "..") {
        throw "Source manifest contains an invalid relative path."
      }
      if ($hash -notmatch "^[0-9a-f]{64}$" -or $bytes -lt 0) {
        throw "Source manifest contains invalid file metadata."
      }
      if ($null -ne $previousPath -and [string]::CompareOrdinal($previousPath, $path) -ge 0) {
        throw "Source manifest paths are not strictly ordered and unique."
      }
      $previousPath = $path
      $totalBytes += $bytes
      if ($totalBytes -gt $script:MaxManifestSourceBytes) {
        throw "Source manifest exceeds its $($script:MaxManifestSourceBytes)-byte source limit."
      }
      $record = [Text.Encoding]::UTF8.GetBytes("$path`0$hash`0$bytes`n")
      $stream.Write($record, 0, $record.Length)
    }
    if ([long]$Manifest.totalBytes -ne $totalBytes) {
      throw "Source manifest byte count does not match its entries."
    }
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
      $stream.Position = 0
      return ([BitConverter]::ToString($sha.ComputeHash($stream))).Replace("-", "").ToLowerInvariant()
    } finally {
      $sha.Dispose()
    }
  } finally {
    $stream.Dispose()
  }
}

function Assert-FootprintExecutableHash {
  param(
    [Parameter(Mandatory)][string]$ExecutablePath,
    [Parameter(Mandatory)][string]$ExpectedExecutableSha256
  )
  $actual = (Get-FileHash -LiteralPath $ExecutablePath -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($actual -ne $ExpectedExecutableSha256.ToLowerInvariant()) {
    throw "Candidate executable changed before launch."
  }
  return $true
}

function Get-FootprintScenarioUiaEvidence {
  param(
    [Parameter(Mandatory)][IntPtr]$WindowHandle,
    [Parameter(Mandatory)][int]$ExpectedProcessId,
    [Parameter(Mandatory)][ValidateSet("chat", "explorer")][string]$Scenario
  )

  $contract = Get-FootprintScenarioContract $Scenario
  $snapshot = Get-FootprintPreferredUiaSnapshot $WindowHandle $ExpectedProcessId
  if ([int]$snapshot.chatCenterCount -ne 1) { throw "Measured $($snapshot.uiaView) UIA tree does not contain exactly one Chat center." }
  if ([int]$snapshot.explorerSidebarCount -ne [int]$contract.explorerSidebarCount) {
    if ($Scenario -eq "chat") {
      throw "Measured Chat state retained an Explorer heavy view; Chat requires that sidebar to be unmounted."
    }
    throw "Measured UIA tree does not contain exactly one active Explorer sidebar."
  }
  return [ordered]@{
    scenario = $Scenario
    uiaView = $snapshot.uiaView
    semanticVerified = $true
    processId = $ExpectedProcessId
    rootTitle = $snapshot.rootTitle
    visitedElementCount = $snapshot.visitedElementCount
    namedElementCount = $snapshot.namedElementCount
    chatCenterCount = $snapshot.chatCenterCount
    explorerSidebarCount = $snapshot.explorerSidebarCount
    explorerHeavyViewMounted = [int]$snapshot.explorerSidebarCount -eq 1
  }
}

function Invoke-FootprintMeasuredScenarioTransition {
  param(
    [Parameter(Mandatory)][IntPtr]$WindowHandle,
    [Parameter(Mandatory)][int]$ExpectedProcessId,
    [Parameter(Mandatory)][ValidateSet("chat", "explorer")][string]$Scenario,
    [ValidateRange(1, 30)][int]$TimeoutSeconds = 10
  )
  if ($Scenario -eq "explorer") {
    $evidence = Wait-FootprintScenarioUiaEvidence $WindowHandle $ExpectedProcessId $Scenario $TimeoutSeconds
    return [ordered]@{ action = "verify-explorer"; invokeCount = 0; providerShowControlExposed = $false; uiaView = $evidence.uiaView; afterEvidence = $evidence }
  }
  $readiness = Wait-FootprintChatTransitionDecision `
    { Get-FootprintChatTransitionSnapshot $WindowHandle $ExpectedProcessId } `
    ($TimeoutSeconds * 10) 100
  $decision = $readiness.decision
  if ([int]$decision.invokeCount -eq 1) {
    $pattern = $null
    $hideButton = $readiness.snapshot.hideSidebarButton
    if ($null -eq $hideButton -or
        -not $hideButton.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref]$pattern) -or $null -eq $pattern) {
      throw "The exact Chat sidebar control does not support the UI Automation invoke pattern."
    }
    ([Windows.Automation.InvokePattern]$pattern).Invoke()
  }
  $evidence = Wait-FootprintScenarioUiaEvidence $WindowHandle $ExpectedProcessId "chat" $TimeoutSeconds
  return [ordered]@{
    action = $decision.action
    invokeCount = [int]$decision.invokeCount
    providerShowControlExposed = [int]$readiness.snapshot.showSidebarButtonCount -eq 1
    uiaView = $readiness.snapshot.uiaView
    readinessAttempts = $readiness.attempts
    afterEvidence = $evidence
  }
}

function Wait-FootprintScenarioUiaEvidence {
  param(
    [Parameter(Mandatory)][IntPtr]$WindowHandle,
    [Parameter(Mandatory)][int]$ExpectedProcessId,
    [Parameter(Mandatory)][ValidateSet("chat", "explorer")][string]$Scenario,
    [ValidateRange(1, 30)][int]$TimeoutSeconds = 10
  )
  $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  do {
    try {
      return Get-FootprintScenarioUiaEvidence $WindowHandle $ExpectedProcessId $Scenario
    } catch {
      $lastError = $_.Exception.Message
      Start-Sleep -Milliseconds 100
    }
  } while ([DateTime]::UtcNow -lt $deadline)
  throw "Measured semantic state was not proven: $lastError"
}

function Assert-FootprintScenarioAcceptance {
  param(
    [Parameter(Mandatory)][ValidateSet("chat", "explorer")][string]$Scenario,
    [Parameter(Mandatory)][Collections.IDictionary]$Setup,
    [Parameter(Mandatory)][Collections.IDictionary]$Measured,
    [Parameter(Mandatory)][object[]]$Samples,
    [Parameter(Mandatory)][string]$ExpectedExecutablePath,
    [Parameter(Mandatory)][string]$ExpectedExecutableSha256,
    [Parameter(Mandatory)][string]$ExpectedSourceAggregateSha256,
    [Parameter(Mandatory)][string]$ExpectedDiagnosticDataDirectorySha256,
    [Parameter(Mandatory)][string]$ExpectedFixtureProjectRootSha256
  )

  $contract = Get-FootprintScenarioContract $Scenario
  foreach ($name in @("executed", "semanticVerified", "processId", "title", "origin", "executablePath", "executableSha256", "sourceAggregateSha256", "diagnosticDataDirectorySha256", "fixtureProjectRootSha256", "projectChatCount", "chatCenterCount", "explorerSidebarCount", "explorerHeavyViewMounted")) {
    if (-not $Setup.Contains($name)) { throw "Setup evidence is missing '$name'." }
  }
  foreach ($name in @("semanticVerified", "processId", "rootTitle", "uiaView", "chatCenterCount", "explorerSidebarCount", "explorerHeavyViewMounted", "transitionAction", "transitionInvokeCount", "transitionShowControlExposed", "transitionUiaView")) {
    if (-not $Measured.Contains($name)) { throw "Measured evidence is missing '$name'." }
  }
  if (-not $Setup.executed -or -not $Setup.semanticVerified -or $Setup.title -ne $contract.exactTitle -or $Setup.origin -ne $contract.exactOrigin) {
    throw "Instrumented scenario setup was not semantically verified."
  }
  if (-not [string]::Equals([IO.Path]::GetFullPath($Setup.executablePath), [IO.Path]::GetFullPath($ExpectedExecutablePath), [StringComparison]::OrdinalIgnoreCase)) {
    throw "Setup used a different executable path."
  }
  if ($Setup.executableSha256 -ne $ExpectedExecutableSha256 -or $Setup.sourceAggregateSha256 -ne $ExpectedSourceAggregateSha256) {
    throw "Setup candidate/source binding does not match acceptance inputs."
  }
  if ($Setup.diagnosticDataDirectorySha256 -ne $ExpectedDiagnosticDataDirectorySha256) {
    throw "Setup did not use the expected isolated diagnostics data directory."
  }
  if ($Setup.fixtureProjectRootSha256 -ne $ExpectedFixtureProjectRootSha256 -or [int]$Setup.projectChatCount -ne 1) {
    throw "Instrumented setup did not prove exactly one native-seeded project Chat."
  }
  if ([int]$Setup.chatCenterCount -ne 1 -or
      [int]$Setup.explorerSidebarCount -ne [int]$contract.explorerSidebarCount -or
      [bool]$Setup.explorerHeavyViewMounted -ne [bool]$contract.explorerHeavyViewMounted) {
    throw "Instrumented setup did not prove the requested mounted-view state."
  }
  if ([int]$Setup.processId -eq [int]$Measured.processId) { throw "Setup and measured launches must be distinct processes." }
  if (-not $Measured.semanticVerified -or $Measured.rootTitle -ne $contract.exactTitle -or [int]$Measured.chatCenterCount -ne 1) {
    throw "Measured Chat center was not proven through UI Automation."
  }
  if ($Measured.uiaView -notin @("control", "raw") -or $Measured.transitionUiaView -notin @("control", "raw")) {
    throw "Measured scenario did not record a supported exact UIA view."
  }
  if ([int]$Measured.explorerSidebarCount -ne [int]$contract.explorerSidebarCount -or
      [bool]$Measured.explorerHeavyViewMounted -ne [bool]$contract.explorerHeavyViewMounted) {
    if ($Scenario -eq "chat") {
      throw "Measured Chat state retained the Explorer heavy view."
    }
    throw "Measured Explorer sidebar was not proven through UI Automation."
  }
  $validTransition = if ($Scenario -eq "chat") {
    ($Measured.transitionAction -eq "invoke-hide-right-sidebar" -and [int]$Measured.transitionInvokeCount -eq 1 -and -not [bool]$Measured.transitionShowControlExposed) -or
    ($Measured.transitionAction -eq "already-closed-noop" -and [int]$Measured.transitionInvokeCount -eq 0 -and [bool]$Measured.transitionShowControlExposed) -or
    ($Measured.transitionAction -eq "already-closed-provider-unexposed" -and [int]$Measured.transitionInvokeCount -eq 0 -and -not [bool]$Measured.transitionShowControlExposed)
  } else {
    $Measured.transitionAction -eq "verify-explorer" -and [int]$Measured.transitionInvokeCount -eq 0 -and -not [bool]$Measured.transitionShowControlExposed
  }
  if (-not $validTransition) {
    throw "Measured scenario did not prove its exact-process semantic transition."
  }
  foreach ($sample in $Samples) {
    if ([int]$sample.unknownProcessCount -ne 0) { throw "Measured tree contains an unknown descendant." }
    Assert-FootprintDebugStateClear @($sample.processes)
  }
  return $true
}

Export-ModuleMember -Function @(
  "Assert-FootprintDiagnosticCampaignInputs",
  "Assert-FootprintDiagnosticEnvironmentClear",
  "Assert-FootprintFixtureEnvironmentClear",
  "Assert-FootprintDiagnosticStateChanged",
  "Assert-FootprintFreshDiagnosticData",
  "Assert-FootprintNormalDataUnchanged",
  "Assert-FootprintExecutableHash",
  "Assert-FootprintDebugStateClear",
  "Assert-FootprintScenarioAcceptance",
  "Get-FootprintScenarioContract",
  "Get-FootprintChatTransitionDecision",
  "Select-FootprintUiaViewSnapshot",
  "Wait-FootprintChatTransitionDecision",
  "Get-FootprintDiagnosticDataProvenance",
  "Get-FootprintFixtureProjectProvenance",
  "Get-FootprintNormalDataInventory",
  "Get-FootprintSourceManifestAggregate",
  "Get-FootprintScenarioUiaEvidence",
  "Invoke-FootprintMeasuredScenarioTransition",
  "Invoke-FootprintBoundedProcess",
  "Invoke-FootprintNormalDataGuardedAction",
  "Test-FootprintCandidateBinding",
  "Wait-FootprintScenarioUiaEvidence"
)
