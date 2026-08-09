Set-StrictMode -Version Latest

Import-Module (Join-Path $PSScriptRoot "DisposableProfile.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "SemanticUia.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "WindowsFootprint.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "ProcessIdentity.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "WindowsJob.psm1") -Force

$script:MaximumRequestBytes = 1MB
$script:States = @("shell", "chat", "file", "diff", "explorer", "source-control")

function Get-C0BObjectKeys {
  param([Parameter(Mandatory)]$Value)
  if ($Value -is [Collections.IDictionary]) { return @($Value.Keys) }
  return @($Value.PSObject.Properties.Name)
}

function Assert-C0BExactKeys {
  param([Parameter(Mandatory)]$Value, [Parameter(Mandatory)][string[]]$Expected, [Parameter(Mandatory)][string]$Label)
  if ($null -eq $Value -or ((Get-C0BObjectKeys $Value | Sort-Object) -join "`0") -ne (($Expected | Sort-Object) -join "`0")) {
    throw "$Label has an invalid shape."
  }
}

function Read-C0BObservationRequest {
  param([Parameter(Mandatory)][string]$Path)
  if (-not [IO.Path]::IsPathRooted($Path)) { throw "Observation request path must be absolute." }
  $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
  if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
      $item.Length -le 0 -or $item.Length -gt $script:MaximumRequestBytes) {
    throw "Observation request must be one bounded regular file."
  }
  $encoding = [Text.UTF8Encoding]::new($false, $true)
  return $encoding.GetString([IO.File]::ReadAllBytes($item.FullName)) | ConvertFrom-Json
}

function Assert-C0BSelector {
  param([Parameter(Mandatory)]$Selector, [Parameter(Mandatory)][string]$Label)
  Assert-C0BExactKeys $Selector @("name", "automationId", "controlType", "view") $Label
  if (([string]::IsNullOrEmpty([string]$Selector.name) -and [string]::IsNullOrEmpty([string]$Selector.automationId)) -or
      $Selector.view -notin @("control", "raw") -or
      $Selector.controlType -notin @("button", "checkbox", "document", "edit", "group", "hyperlink", "listitem", "pane", "tabitem", "text", "tree", "treeitem")) {
    throw "$Label is not an exact bounded semantic selector."
  }
}

function Assert-C0BPlanAction {
  param([Parameter(Mandatory)]$Action, [Parameter(Mandatory)][string]$Label)
  Assert-C0BExactKeys $Action @("selector", "operation", "value", "resultSelector", "expectedValue") $Label
  Assert-C0BSelector $Action.selector "$Label.selector"
  if ($Action.operation -notin @("focus", "invoke", "select", "set-value", "toggle")) { throw "$Label operation is invalid." }
  if ($null -ne $Action.resultSelector) { Assert-C0BSelector $Action.resultSelector "$Label.resultSelector" }
  if ($Action.operation -eq "set-value") {
    if ($null -eq $Action.value -or $null -eq $Action.expectedValue -or $null -ne $Action.resultSelector -or
        [Text.Encoding]::UTF8.GetByteCount([string]$Action.value) -gt 32768 -or
        [Text.Encoding]::UTF8.GetByteCount([string]$Action.expectedValue) -gt 32768) {
      throw "$Label value observation is invalid."
    }
  } elseif ($null -ne $Action.value -or $null -ne $Action.expectedValue -or $null -eq $Action.resultSelector) {
    throw "$Label must bind its semantic result."
  }
}

function Assert-C0BObservationRequest {
  param([Parameter(Mandatory)]$Request)
  Assert-C0BExactKeys $Request @(
    "schemaVersion", "stateId", "participant", "launchOrdinal", "executable", "profile",
    "projectRoot", "expectedWindowTitle", "cadence", "statePlan", "outputPath"
  ) "request"
  Assert-C0BExactKeys $Request.executable @("path", "sha256", "bytes", "bindingSha256") "request.executable"
  Assert-C0BExactKeys $Request.profile @("campaignRoot", "id") "request.profile"
  Assert-C0BExactKeys $Request.cadence @("settleAfterSemanticReadyMs", "snapshotIntervalMs", "snapshotCount") "request.cadence"
  Assert-C0BExactKeys $Request.statePlan @("setupActions", "readySelector", "inputProbe", "tabProbe", "restoreActions") "request.statePlan"
  $executableBytes = 0L
  $validExecutableBytes = [long]::TryParse(
    [string]$Request.executable.bytes,
    [Globalization.NumberStyles]::None,
    [Globalization.CultureInfo]::InvariantCulture,
    [ref]$executableBytes
  )
  if ($Request.schemaVersion -ne 1 -or $Request.stateId -notin $script:States -or
      $Request.participant -notin @("candidate", "control") -or
      $Request.launchOrdinal -lt 1 -or $Request.launchOrdinal -gt 30 -or
      $Request.executable.sha256 -notmatch '^[0-9a-f]{64}$' -or
      -not $validExecutableBytes -or $executableBytes -lt 1 -or
      $Request.executable.bindingSha256 -notmatch '^[0-9a-f]{64}$' -or
      $Request.profile.id -notmatch '^[a-z0-9][a-z0-9-]{0,63}$' -or
      [string]::IsNullOrWhiteSpace([string]$Request.expectedWindowTitle) -or
      [Text.Encoding]::UTF8.GetByteCount([string]$Request.expectedWindowTitle) -gt 128 -or
      $Request.cadence.settleAfterSemanticReadyMs -lt 100 -or $Request.cadence.settleAfterSemanticReadyMs -gt 30000 -or
      $Request.cadence.snapshotIntervalMs -lt 50 -or $Request.cadence.snapshotIntervalMs -gt 10000 -or
      $Request.cadence.snapshotCount -lt 2 -or $Request.cadence.snapshotCount -gt 512) {
    throw "Observation request values are outside their fixed bounds."
  }
  foreach ($path in @($Request.executable.path, $Request.profile.campaignRoot, $Request.outputPath)) {
    if (-not [IO.Path]::IsPathRooted([string]$path)) { throw "Observation request paths must be absolute." }
  }
  if ($null -ne $Request.projectRoot -and -not [IO.Path]::IsPathRooted([string]$Request.projectRoot)) {
    throw "Observation project root must be null or absolute."
  }
  Assert-C0BSelector $Request.statePlan.readySelector "request.statePlan.readySelector"
  foreach ($property in @("setupActions", "restoreActions")) {
    $actions = @($Request.statePlan.$property)
    if ($actions.Count -gt 16) { throw "request.statePlan.$property exceeds 16 actions." }
    for ($index = 0; $index -lt $actions.Count; $index += 1) {
      Assert-C0BPlanAction $actions[$index] "request.statePlan.$property[$index]"
    }
  }
  Assert-C0BPlanAction $Request.statePlan.inputProbe "request.statePlan.inputProbe"
  Assert-C0BPlanAction $Request.statePlan.tabProbe "request.statePlan.tabProbe"
}

function Wait-C0BSelector {
  param([Parameter(Mandatory)][Collections.IDictionary]$Context, [Parameter(Mandatory)]$Selector)
  return Wait-C0BSemanticElement $Context.handle $Context.pid $Context.title `
    $Context.executablePath $Context.creationTimeUtc `
    ([string]$Selector.name) ([string]$Selector.automationId) `
    ([string]$Selector.controlType) ([string]$Selector.view) 10000
}

function Invoke-C0BPlanAction {
  param([Parameter(Mandatory)][Collections.IDictionary]$Context, [Parameter(Mandatory)]$Action)
  $element = Wait-C0BSelector $Context $Action.selector
  $started = (Get-FootprintMonotonicClock).tick
  Invoke-C0BSemanticAction $element ([string]$Action.operation) $Action.value
  if ($Action.operation -eq "set-value") {
    [void](Wait-C0BSemanticValue $Context.handle $Context.pid $Context.title `
      $Context.executablePath $Context.creationTimeUtc `
      ([string]$Action.selector.name) ([string]$Action.selector.automationId) `
      ([string]$Action.selector.controlType) ([string]$Action.expectedValue) `
      ([string]$Action.selector.view) 10000)
  } else {
    [void](Wait-C0BSelector $Context $Action.resultSelector)
  }
  return [ordered]@{ startedTick = $started; completedTick = (Get-FootprintMonotonicClock).tick }
}

function Assert-C0BExecutable {
  param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory)][string]$Sha256,
    [Parameter(Mandatory)][long]$Bytes
  )
  $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
  if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
      -not [string]::Equals($item.Extension, ".exe", [StringComparison]::OrdinalIgnoreCase) -or
      $item.Length -ne $Bytes -or
      (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Sha256) {
    throw "Observation executable identity is invalid."
  }
  return $item.FullName
}

function Start-C0BExactProcess {
  param([Parameter(Mandatory)]$Request, [Parameter(Mandatory)][string]$ExecutablePath, [Parameter(Mandatory)][string]$ProfileRoot)
  $existing = @(Get-FootprintProcessSnapshot | Where-Object {
    $_.ExecutablePath -and [string]::Equals([IO.Path]::GetFullPath([string]$_.ExecutablePath), $ExecutablePath, [StringComparison]::OrdinalIgnoreCase)
  })
  if ($existing.Count -ne 0) { throw "The exact observation executable is already running." }
  $info = [Diagnostics.ProcessStartInfo]::new()
  $info.FileName = $ExecutablePath
  $info.WorkingDirectory = [IO.Path]::GetDirectoryName($ExecutablePath)
  $info.UseShellExecute = $false
  $info.CreateNoWindow = $false
  foreach ($name in @("SAGE_DISPOSABLE_PROFILE_ROOT", "SAGE_PROJECT_ROOT", "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER")) {
    [void]$info.EnvironmentVariables.Remove($name)
  }
  $info.EnvironmentVariables["SAGE_DISPOSABLE_PROFILE_ROOT"] = $ProfileRoot
  if ($null -ne $Request.projectRoot) { $info.EnvironmentVariables["SAGE_PROJECT_ROOT"] = [string]$Request.projectRoot }
  $launch = Start-FootprintJobProcess $info 33
  return [ordered]@{
    process = $launch.process
    boundary = $launch.boundary
    spawnTick = $launch.spawnStartedTick
  }
}

function Invoke-C0BLaunchCore {
  param([Parameter(Mandatory)]$Request)
  $executable = Assert-C0BExecutable ([string]$Request.executable.path) `
    ([string]$Request.executable.sha256) ([long]$Request.executable.bytes)
  $profile = New-C0BDisposableProfile ([string]$Request.profile.campaignRoot) ([string]$Request.profile.id)
  $process = $null
  $jobBoundary = $null
  $run = $null
  $failure = $null
  $cleaned = $false
  try {
    $launch = Start-C0BExactProcess $Request $executable $profile.root
    $process = $launch.process
    $jobBoundary = $launch.boundary
    $window = Wait-FootprintWindow $process.Id 30 ([string]$Request.expectedWindowTitle)
    $windowReadyTick = (Get-FootprintMonotonicClock).tick
    $identity = Get-FootprintNativeProcessIdentity $process $executable ([string]$Request.executable.bindingSha256)
    $context = [ordered]@{
      handle = $window.handle
      pid = $process.Id
      title = [string]$Request.expectedWindowTitle
      executablePath = $executable
      creationTimeUtc = $identity.creationTimeUtc
    }
    foreach ($action in @($Request.statePlan.setupActions)) { [void](Invoke-C0BPlanAction $context $action) }
    [void](Wait-C0BSelector $context $Request.statePlan.readySelector)
    $inputSpan = Invoke-C0BPlanAction $context $Request.statePlan.inputProbe
    $tabSpan = Invoke-C0BPlanAction $context $Request.statePlan.tabProbe
    foreach ($action in @($Request.statePlan.restoreActions)) { [void](Invoke-C0BPlanAction $context $action) }
    [void](Wait-C0BSelector $context $Request.statePlan.readySelector)
    $stateReadyTick = (Get-FootprintMonotonicClock).tick
    $clock = Get-FootprintMonotonicClock
    $settleTicks = [long][Math]::Ceiling(([double]$Request.cadence.settleAfterSemanticReadyMs * $clock.frequencyHz) / 1000.0)
    $samplingStartedTick = $stateReadyTick + $settleTicks
    $cadenceSnapshots = @(Invoke-FootprintAbsoluteCadence $samplingStartedTick ([int]$Request.cadence.snapshotIntervalMs) `
      ([int]$Request.cadence.snapshotCount) {
        Get-C0BSanitizedTreeSnapshot $process.Id
      })
    $snapshots = @($cadenceSnapshots | ForEach-Object {
      $value = $_.value
      [ordered]@{
        ordinal = [int]$_.ordinal
        dueTick = [long]$_.dueTick
        capturedTick = [long]$_.capturedTick
        membershipSha256 = [string]$value.membershipSha256
        coreProcessCount = [int]$value.coreProcessCount
        unknownProcessCount = [int]$value.unknownProcessCount
        coreCpuTime100ns = [long]$value.coreCpuTime100ns
        corePrivateBytes = [long]$value.corePrivateBytes
        coreWorkingSetBytes = [long]$value.coreWorkingSetBytes
      }
    })
    [void](Assert-C0BExecutable $executable ([string]$Request.executable.sha256) ([long]$Request.executable.bytes))
    $cleanup = Stop-FootprintJobProcess $process $jobBoundary 1000 5000
    if (-not $cleanup.exited -or -not $cleanup.treeBoundaryVerified) { throw "Observation process teardown was not verified." }
    $jobBoundary.Dispose()
    $jobBoundary = $null
    $cleaned = $true
    $run = [ordered]@{
      launchOrdinal = [int]$Request.launchOrdinal
      pid = $identity.pid
      creationTimeUtc = $identity.creationTimeUtc
      creationTimeFiletime100ns = $identity.creationTimeFiletime100ns
      processIdentitySha256 = $identity.processIdentitySha256
      executableBindingSha256 = [string]$Request.executable.bindingSha256
      disposableProfileIdentitySha256 = $profile.identitySha256
      spawnStartedTick = $launch.spawnTick
      windowReadyTick = $windowReadyTick
      stateReadyTick = $stateReadyTick
      samplingStartedTick = $samplingStartedTick
      stateVerified = $true
      inputReceivedTick = $inputSpan.startedTick
      inputPaintedTick = $inputSpan.completedTick
      tabActivatedTick = $tabSpan.startedTick
      tabPaintedTick = $tabSpan.completedTick
      spawnToWindowReadyUs = Get-FootprintMonotonicDurationUs $launch.spawnTick $windowReadyTick $clock.frequencyHz
      inputToPaintUs = Get-FootprintMonotonicDurationUs $inputSpan.startedTick $inputSpan.completedTick $clock.frequencyHz
      tabSwitchUs = Get-FootprintMonotonicDurationUs $tabSpan.startedTick $tabSpan.completedTick $clock.frequencyHz
      snapshots = $snapshots
      closeRequestedTick = [long]$cleanup.closeRequestedTick
      zeroDescendantsTick = [long]$cleanup.zeroDescendantsTick
      normalSageDataMutations = 0
      orphanProcessesAfterClose = [int]$cleanup.orphanProcessesAfterClose
    }
  } catch {
    $failure = $_
  }
  $cleanupFailure = $null
  if ($null -ne $process -and $null -ne $jobBoundary -and -not $cleaned) {
    try { [void](Stop-FootprintJobProcess $process $jobBoundary 0 5000) } catch { $cleanupFailure = $_ }
    finally {
      $jobBoundary.Dispose()
      $jobBoundary = $null
    }
  }
  try { Remove-C0BDisposableProfile ([string]$Request.profile.campaignRoot) $profile.root } catch {
    if ($null -eq $cleanupFailure) { $cleanupFailure = $_ }
  }
  if ($null -ne $cleanupFailure) {
    if ($null -ne $failure) { throw "Observation failed: $($failure.Exception.Message) Cleanup failed: $($cleanupFailure.Exception.Message)" }
    throw $cleanupFailure
  }
  if ($null -ne $failure) { throw $failure }
  return $run
}

function Write-C0BObservationResult {
  param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)]$Value)
  $parent = Get-Item -LiteralPath ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($Path))) -Force -ErrorAction Stop
  if (-not $parent.PSIsContainer -or ($parent.Attributes -band [IO.FileAttributes]::ReparsePoint) -or (Test-Path -LiteralPath $Path)) {
    throw "Observation result path must be a new file below a plain directory."
  }
  $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($Value | ConvertTo-Json -Compress -Depth 12))
  if ($bytes.Length -le 0 -or $bytes.Length -gt 1MB) { throw "Observation result exceeds its byte bound." }
  $stream = [IO.FileStream]::new($Path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
  try { $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) } finally { $stream.Dispose() }
}

function Invoke-C0BObservationRequest {
  param([Parameter(Mandatory)][string]$RequestPath)
  $request = Read-C0BObservationRequest $RequestPath
  Assert-C0BObservationRequest $request
  $normalBefore = Get-FootprintNormalDataInventory
  $guarded = Invoke-FootprintNormalDataGuardedAction $normalBefore `
    { Invoke-C0BLaunchCore $request } `
    { Get-FootprintNormalDataInventory }
  Write-C0BObservationResult ([string]$request.outputPath) $guarded.result
  return [ordered]@{ status = "passed"; stateId = [string]$request.stateId; participant = [string]$request.participant; launchOrdinal = [int]$request.launchOrdinal }
}

Export-ModuleMember -Function "Invoke-C0BObservationRequest"
