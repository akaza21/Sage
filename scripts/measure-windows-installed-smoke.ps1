param(
  [Parameter(Mandatory)][string]$InstallerPath,
  [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{64}$')][string]$InstallerSha256,
  [Parameter(Mandatory)][ValidateRange(1, 536870912)][long]$InstallerBytes,
  [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedInstalledExecutableSha256,
  [Parameter(Mandatory)][ValidateRange(1, 536870912)][long]$ExpectedInstalledExecutableBytes,
  [Parameter(Mandatory)][ValidatePattern('^[0-9a-fA-F]{40}$')][string]$ExpectedSignerThumbprint,
  [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{64}$')][string]$PackageReceiptSha256,
  [Parameter(Mandatory)][ValidatePattern('^[a-z0-9][a-z0-9-]{0,63}$')][string]$CampaignId,
  [Parameter(Mandatory)][string]$OutputDirectory,
  [Parameter(Mandatory)][string]$NodePath,
  [Parameter(Mandatory)][string]$GitPath,
  [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{64}$')][string]$GitSha256,
  [Parameter(Mandatory)][ValidateRange(1, 536870912)][long]$GitBytes
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

Import-Module (Join-Path $PSScriptRoot "footprint/DisposableProfile.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "footprint/ScenarioEvidence.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "footprint/WindowsFootprint.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "footprint/WindowsJob.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "footprint/ProcessIdentity.psm1") -Force

$stepIds = @(
  "install-initial", "launch-initial", "drive-primary", "close-initial",
  "launch-restart", "verify-restart", "close-restart", "uninstall-initial",
  "install-reinstall", "launch-reinstall", "verify-reinstall",
  "close-reinstall", "uninstall-final"
)
$assertionNames = @(
  "projectSeeded", "fileOpened", "fileEditedAndSaved", "workspaceSearchMatched",
  "gitStaged", "gitCommitted", "chatCreated", "chatRenamed", "chatSwitched",
  "simulatorTurnCompleted", "restartPersisted", "reinstallVerified"
)
$runner = (Resolve-Path (Join-Path $PSScriptRoot "footprint/bounded-node-runner.mjs")).Path
$driver = (Resolve-Path (Join-Path $PSScriptRoot "footprint/drive-installed-smoke.mjs")).Path
$steps = @($stepIds | ForEach-Object {
  [ordered]@{ id = $_; status = "skipped"; durationMs = 0; evidenceSha256 = $null }
})
$assertions = [ordered]@{}
foreach ($name in $assertionNames) { $assertions[$name] = $false }
$currentStep = "prerequisites"
$failure = $null
$result = "failed"
$started = [DateTime]::UtcNow.ToString("o")
$profile = $null
$job = $null
$installed = $false

function Get-StringSha256 {
  param([Parameter(Mandatory)][string]$Value)
  $hasher = [Security.Cryptography.SHA256]::Create()
  try {
    return ([BitConverter]::ToString($hasher.ComputeHash([Text.Encoding]::UTF8.GetBytes($Value)))).Replace("-", "").ToLowerInvariant()
  } finally { $hasher.Dispose() }
}

function Test-PlainDirectory {
  param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$Label)
  $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
  if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw "$Label must be a plain directory."
  }
  return $item.FullName
}

function Test-FileIdentity {
  param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory)][long]$Bytes,
    [Parameter(Mandatory)][string]$Sha256
  )
  try {
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    return -not $item.PSIsContainer -and
      -not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -and
      $item.Length -eq $Bytes -and
      (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() -eq $Sha256
  } catch { return $false }
}

function Assert-GitIdentity {
  if (-not (Test-FileIdentity $resolvedGit $GitBytes $GitSha256)) {
    throw "Authenticated Git identity changed during smoke execution."
  }
}

function Invoke-Bounded {
  param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string[]]$Arguments,
    [Parameter(Mandatory)][string]$Label,
    [ValidateRange(100, 120000)][int]$TimeoutMs = 120000
  )
  if ([string]::Equals($Executable, $resolvedGit, [StringComparison]::OrdinalIgnoreCase)) {
    Assert-GitIdentity
  }
  $resultPath = Join-Path $resolvedOutput ("process-{0:D2}-{1}.json" -f ($script:processOrdinal += 1), $Label)
  try {
    return Invoke-FootprintBoundedProcess $resolvedNode $runner $Executable $Arguments $resultPath $TimeoutMs 65536
  } finally {
    if (Test-Path -LiteralPath $resultPath -PathType Leaf) { Remove-Item -LiteralPath $resultPath -Force }
  }
}

function Invoke-BoundedInstallerProcess {
  param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string[]]$Arguments,
    [ValidateRange(1, 120)][int]$TimeoutSeconds = 120
  )
  $process = Start-Process -FilePath $Executable -ArgumentList $Arguments -PassThru -WindowStyle Hidden
  $identity = $null
  $known = New-FootprintIdentityStore
  $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  try {
    $identity = Get-FootprintLaunchIdentity $process $Executable
    [void](Add-FootprintRememberedIdentity $known ([pscustomobject]@{
      ProcessId = $identity.processId; ExecutablePath = $identity.executablePath; CreationDate = $identity.creationDate
    }))
    do {
      foreach ($row in @(Get-FootprintDescendants $process.Id @(Get-FootprintProcessSnapshot))) {
        [void](Add-FootprintRememberedIdentity $known $row)
      }
      $process.Refresh()
      if ($process.HasExited) { break }
      Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $deadline)
    if (-not $process.HasExited) { throw "Installer process timed out." }
    $exitCode = $process.ExitCode
  } catch {
    if ($null -eq $identity) {
      $fallback = Stop-FootprintLaunchedProcessObject $process $Executable $known
      if (-not $fallback.exited -or -not $fallback.treeBoundaryVerified) {
        throw "Installer identity acquisition and exact fallback cleanup both failed."
      }
    }
    throw
  } finally {
    if ($null -ne $identity) { $teardown = Stop-FootprintTree $identity $known 30 }
  }
  if ($teardown.identityLost -or $teardown.remaining.Count -ne 0 -or $teardown.forced) {
    throw "Installer process-tree identity did not converge naturally."
  }
  if ($exitCode -ne 0) { throw "Installer process exited with code $exitCode." }
  return [ordered]@{ exitCode = $exitCode; zeroOrphans = $true }
}

function Set-StepPassed {
  param([Parameter(Mandatory)][string]$Id, [Parameter(Mandatory)][Diagnostics.Stopwatch]$Timer, [Parameter(Mandatory)]$Evidence)
  $index = [Array]::IndexOf($stepIds, $Id)
  $steps[$index] = [ordered]@{
    id = $Id
    status = "passed"
    durationMs = [Math]::Min(600000, [Math]::Max(0, [int]$Timer.ElapsedMilliseconds))
    evidenceSha256 = Get-StringSha256 ($Evidence | ConvertTo-Json -Compress -Depth 6)
  }
}

function Set-StepFailed {
  param([Parameter(Mandatory)][string]$Id)
  $index = [Array]::IndexOf($stepIds, $Id)
  if ($index -ge 0) {
    $steps[$index] = [ordered]@{ id = $Id; status = "failed"; durationMs = 0; evidenceSha256 = $null }
  }
}

function Get-AvailablePort {
  $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
  try {
    $listener.Start()
    return ([Net.IPEndPoint]$listener.LocalEndpoint).Port
  } finally { $listener.Stop() }
}

function Invoke-Install {
  param([Parameter(Mandatory)][string]$StepId)
  $script:currentStep = $StepId
  $timer = [Diagnostics.Stopwatch]::StartNew()
  $script:installed = $true
  $processEvidence = Invoke-BoundedInstallerProcess $resolvedInstaller @("/S", "/D=$installRoot")
  $executable = Join-Path $installRoot "sage.exe"
  $uninstaller = Join-Path $installRoot "uninstall.exe"
  foreach ($file in @($executable, $uninstaller)) {
    $item = Get-Item -LiteralPath $file -Force -ErrorAction Stop
    if (-not $item.PSIsContainer -and -not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { continue }
    throw "Installed payload identity is invalid."
  }
  $installedItem = Get-Item -LiteralPath $executable -Force
  $installedSha256 = (Get-FileHash $executable -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($installedItem.Length -ne $ExpectedInstalledExecutableBytes -or $installedSha256 -ne $ExpectedInstalledExecutableSha256) {
    throw "Installed executable does not match the authenticated package projection."
  }
  Set-StepPassed $StepId $timer ([ordered]@{
    executableSha256 = $installedSha256; executableBytes = [long]$installedItem.Length
    zeroOrphans = $processEvidence.zeroOrphans
  })
  return [ordered]@{ executable = $executable; uninstaller = $uninstaller }
}

function Invoke-Uninstall {
  param([Parameter(Mandatory)][string]$StepId, [Parameter(Mandatory)][string]$Uninstaller)
  $script:currentStep = $StepId
  $timer = [Diagnostics.Stopwatch]::StartNew()
  $processEvidence = Invoke-BoundedInstallerProcess $Uninstaller @("/S")
  if (Test-Path -LiteralPath (Join-Path $installRoot "sage.exe")) { throw "Uninstall retained the installed executable." }
  if (Test-Path -LiteralPath $installRoot -PathType Container) {
    [void](Test-PlainDirectory $installRoot "Installed payload root")
    if (@(Get-ChildItem -LiteralPath $installRoot -Force | Select-Object -First 1).Count -ne 0) {
      throw "Uninstall retained installed payload entries."
    }
    Remove-Item -LiteralPath $installRoot -Force
  }
  $script:installed = $false
  Set-StepPassed $StepId $timer ([ordered]@{ executableAbsent = $true; zeroOrphans = $processEvidence.zeroOrphans })
}

function Invoke-SemanticLaunch {
  param(
    [Parameter(Mandatory)][string]$LaunchStep,
    [Parameter(Mandatory)][string]$DriveStep,
    [Parameter(Mandatory)][string]$CloseStep,
    [Parameter(Mandatory)][ValidateSet("primary", "restart", "reinstall")][string]$Mode,
    [Parameter(Mandatory)][string]$Executable
  )
  Assert-GitIdentity
  $script:currentStep = $LaunchStep
  $launchTimer = [Diagnostics.Stopwatch]::StartNew()
  $launchItem = Get-Item -LiteralPath $Executable -Force
  if ($launchItem.Length -ne $ExpectedInstalledExecutableBytes -or
      (Get-FileHash $Executable -Algorithm SHA256).Hash.ToLowerInvariant() -ne $ExpectedInstalledExecutableSha256) {
    throw "Installed executable identity changed before launch."
  }
  $port = Get-AvailablePort
  $start = [Diagnostics.ProcessStartInfo]::new()
  $start.FileName = $Executable
  $start.WorkingDirectory = $installRoot
  $start.UseShellExecute = $false
  $start.CreateNoWindow = $true
  foreach ($name in @(
    "SAGE_PROJECT_ROOT", "SAGE_DIAGNOSTIC_DATA_DIR",
    "SAGE_CHARACTERIZATION_FIXTURE_PATH", "SAGE_CHARACTERIZATION_FIXTURE_SHA256",
    "SAGE_CHARACTERIZATION_FIXTURE_BYTES", "SAGE_CHARACTERIZATION_CASE"
  )) { [void]$start.EnvironmentVariables.Remove($name) }
  $start.EnvironmentVariables["SAGE_DISPOSABLE_PROFILE_ROOT"] = $profile.root
  if ($Mode -eq "primary") { $start.EnvironmentVariables["SAGE_PROJECT_ROOT"] = $fixtureRoot }
  else { [void]$start.EnvironmentVariables.Remove("SAGE_PROJECT_ROOT") }
  $start.EnvironmentVariables["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = "--remote-debugging-port=$port"
  $start.EnvironmentVariables["WEBVIEW2_BROWSER_EXECUTABLE_FOLDER"] = ""
  $start.EnvironmentVariables["PATH"] = "{0};{1}" -f ([IO.Path]::GetDirectoryName($resolvedGit)), (Join-Path $env:SystemRoot "System32")
  $start.EnvironmentVariables["GIT_CONFIG_NOSYSTEM"] = "1"
  $start.EnvironmentVariables["GIT_CONFIG_GLOBAL"] = "NUL"
  $script:job = Start-FootprintJobProcess $start
  try {
    $window = Wait-FootprintWindow $job.process.Id 20
    Set-StepPassed $LaunchStep $launchTimer ([ordered]@{ windowTitleSha256 = Get-StringSha256 $window.title })
    $script:currentStep = $DriveStep
    $driveTimer = [Diagnostics.Stopwatch]::StartNew()
    $semanticPath = Join-Path $resolvedOutput ("semantic-{0}.json" -f $Mode)
    if (Test-Path -LiteralPath $semanticPath) { throw "Semantic evidence path is not fresh." }
    $nodeResult = Invoke-Bounded $resolvedNode @($driver, [string]$port, $Mode, $rootLabel, $semanticPath) "drive-$Mode" 60000
    $semantic = Get-Content -LiteralPath $semanticPath -Raw | ConvertFrom-Json
    $semanticBytes = (Get-Item -LiteralPath $semanticPath).Length
    if ($semanticBytes -gt 32768 -or [int]$semantic.schemaVersion -ne 1 -or [string]$semantic.mode -ne $Mode) {
      throw "Semantic evidence is invalid."
    }
    foreach ($name in $assertionNames) {
      if ([bool]$semantic.assertions.$name) { $assertions[$name] = $true }
    }
    Set-StepPassed $DriveStep $driveTimer ([ordered]@{ stdoutSha256 = Get-StringSha256 $nodeResult.stdout; semanticSha256 = (Get-FileHash $semanticPath -Algorithm SHA256).Hash.ToLowerInvariant() })
  } finally {
    $script:currentStep = $CloseStep
    $closeTimer = [Diagnostics.Stopwatch]::StartNew()
    if ($null -ne $job) {
      $teardown = Stop-FootprintJobProcess $job.process $job.boundary 1000 5000
      $job = $null
      if (-not $teardown.treeBoundaryVerified -or $teardown.orphanProcessesAfterClose -ne 0) { throw "Launch cleanup identity was not verified." }
      Set-StepPassed $CloseStep $closeTimer ([ordered]@{ forced = [bool]$teardown.forced; zeroOrphans = $true })
    }
  }
}

function New-FixtureRepository {
  [void][IO.Directory]::CreateDirectory($fixtureRoot)
  [IO.File]::WriteAllText((Join-Path $fixtureRoot "README.md"), "sage installed smoke initial`n", [Text.UTF8Encoding]::new($false))
  foreach ($command in @(
    @("init"), @("config", "user.name", "Sage Smoke"),
    @("config", "user.email", "sage-smoke@example.invalid"),
    @("add", "README.md"), @("commit", "-m", "initial fixture")
  )) { [void](Invoke-Bounded $resolvedGit (@("-C", $fixtureRoot) + $command) "git-fixture" 30000) }
  $head = (Invoke-Bounded $resolvedGit @("-C", $fixtureRoot, "rev-parse", "HEAD") "git-head" 30000).stdout.Trim()
  return [ordered]@{
    rootIdentitySha256 = Get-StringSha256 ([IO.Path]::GetFullPath($fixtureRoot).TrimEnd('\').ToLowerInvariant())
    rootLabel = $rootLabel
    initialCommitSha256 = Get-StringSha256 $head
  }
}

if ($env:OS -ne "Windows_NT") { throw "This smoke harness requires Windows." }
$parent = Split-Path -Parent ([IO.Path]::GetFullPath($OutputDirectory))
if (-not (Test-Path -LiteralPath $parent -PathType Container)) { throw "Smoke output parent is missing." }
[void](Test-PlainDirectory $parent "Smoke output parent")
if (-not (Test-Path -LiteralPath $OutputDirectory -PathType Container)) { throw "Smoke output must be reserved." }
$resolvedOutput = Test-PlainDirectory ([IO.Path]::GetFullPath($OutputDirectory)) "Smoke output"
if (@(Get-ChildItem -LiteralPath $resolvedOutput -Force).Count -ne 0) { throw "Smoke output must be empty." }
$resolvedInstaller = (Resolve-Path -LiteralPath $InstallerPath).Path
$resolvedNode = (Resolve-Path -LiteralPath $NodePath).Path
$resolvedGit = (Resolve-Path -LiteralPath $GitPath).Path
$script:processOrdinal = 0
$installRoot = Join-Path $resolvedOutput "installed"
$profileCampaignRoot = [IO.Directory]::CreateDirectory((Join-Path $resolvedOutput "profiles")).FullName
$rootLabel = "sage-installed-smoke-fixture"
$fixtureRoot = Join-Path $resolvedOutput $rootLabel
$receiptPath = Join-Path $resolvedOutput "receipt.json"
$signature = Get-AuthenticodeSignature -LiteralPath $resolvedInstaller
$webViewVersion = @(
  @(
    Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F1E7E89C-98C0-4E8A-8B72-6AD3D4C13C4B}" -ErrorAction SilentlyContinue
    Get-ItemProperty "HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F1E7E89C-98C0-4E8A-8B72-6AD3D4C13C4B}" -ErrorAction SilentlyContinue
  ) | Where-Object { $_.pv } | Select-Object -First 1 -ExpandProperty pv
)
$administrator = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$gitIdentity = Test-FileIdentity $resolvedGit $GitBytes $GitSha256
$gitVersion = $null
if ($gitIdentity) {
  try { $gitVersion = (Invoke-Bounded $resolvedGit @("--version") "git-version" 30000).stdout.Trim().Replace("git version ", "") } catch {}
}
$prerequisites = [ordered]@{
  windows = $true
  interactiveSession = [Environment]::UserInteractive -and $env:SESSIONNAME -notmatch '^Services$'
  webView2 = $webViewVersion.Count -eq 1
  administrator = $administrator
  authenticode = $signature.Status -eq [Management.Automation.SignatureStatus]::Valid -and
    $null -ne $signature.SignerCertificate -and
    $signature.SignerCertificate.Thumbprint.ToLowerInvariant() -eq $ExpectedSignerThumbprint.ToLowerInvariant()
  git = $gitIdentity -and -not [string]::IsNullOrWhiteSpace($gitVersion)
  osVersion = [Environment]::OSVersion.Version.ToString()
  webView2Version = if ($webViewVersion.Count -eq 1) { [string]$webViewVersion[0] } else { $null }
  gitVersion = $gitVersion
  signerThumbprint = if ($null -ne $signature.SignerCertificate) { $signature.SignerCertificate.Thumbprint.ToLowerInvariant() } else { $null }
}
$fixture = [ordered]@{ rootIdentitySha256 = "0" * 64; rootLabel = $rootLabel; initialCommitSha256 = "0" * 64 }

try {
  $installer = Get-Item -LiteralPath $resolvedInstaller -Force
  if ($installer.PSIsContainer -or ($installer.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
      $installer.Length -ne $InstallerBytes -or
      (Get-FileHash -LiteralPath $resolvedInstaller -Algorithm SHA256).Hash.ToLowerInvariant() -ne $InstallerSha256) {
    throw "Authenticated installer identity changed before smoke execution."
  }
  $missing = @(
    @("windows", "windows-unavailable"), @("interactiveSession", "interactive-session-unavailable"),
    @("webView2", "webview2-unavailable"), @("administrator", "administrator-unavailable"),
    @("authenticode", "installer-signature-invalid"), @("git", "git-unavailable")
  ) | Where-Object { -not [bool]$prerequisites[$_[0]] } | Select-Object -First 1
  if ($null -ne $missing) {
    $result = "blocked"
    $failure = [ordered]@{ code = $missing[1]; phase = "prerequisites" }
  } else {
    $fixture = New-FixtureRepository
    $profile = New-C0BDisposableProfile $profileCampaignRoot "installed-smoke"
    $payload = Invoke-Install "install-initial"
    Invoke-SemanticLaunch "launch-initial" "drive-primary" "close-initial" "primary" $payload.executable
    Invoke-SemanticLaunch "launch-restart" "verify-restart" "close-restart" "restart" $payload.executable
    Invoke-Uninstall "uninstall-initial" $payload.uninstaller
    $payload = Invoke-Install "install-reinstall"
    Invoke-SemanticLaunch "launch-reinstall" "verify-reinstall" "close-reinstall" "reinstall" $payload.executable
    Invoke-Uninstall "uninstall-final" $payload.uninstaller
    if (@($assertions.Values | Where-Object { -not $_ }).Count -ne 0) { throw "Installed smoke assertions are incomplete." }
    $result = "passed"
  }
} catch {
  Set-StepFailed $currentStep
  $code = if ($_.Exception.Message -match 'timeout|exceeded') { "step-timeout" } elseif ($_.Exception.Message -match 'identity|orphan|cleanup') { "identity-lost" } else { "step-failed" }
  $failure = [ordered]@{ code = $code; phase = $currentStep }
  if ($installed -and $null -eq $job) {
    try {
      $candidate = Join-Path $installRoot "uninstall.exe"
      if (Test-Path -LiteralPath $candidate -PathType Leaf) { Invoke-Uninstall "uninstall-final" $candidate }
    } catch {
      Set-StepFailed "uninstall-final"
      $failure = [ordered]@{ code = "cleanup-failed"; phase = "uninstall-final" }
    }
  }
} finally {
  if ($null -ne $job) {
    try { [void](Stop-FootprintJobProcess $job.process $job.boundary 0 5000) } catch { $failure = [ordered]@{ code = "cleanup-failed"; phase = $currentStep } }
  }
  if ($null -ne $profile) {
    try { Remove-C0BDisposableProfile $profileCampaignRoot $profile.root } catch { $failure = [ordered]@{ code = "cleanup-failed"; phase = "profile-cleanup" } }
  }
  if ($null -ne $failure -and $result -ne "blocked") { $result = "failed" }
  $receipt = [ordered]@{
    schemaVersion = 1; workItem = "BETA-0001"; campaignId = $CampaignId; platform = "windows"
    installer = [ordered]@{
      kind = "nsis"; sha256 = $InstallerSha256; bytes = $InstallerBytes
      expectedInstalledExecutableSha256 = $ExpectedInstalledExecutableSha256
      expectedInstalledExecutableBytes = $ExpectedInstalledExecutableBytes
      signerThumbprint = $ExpectedSignerThumbprint.ToLowerInvariant()
    }
    packageReceiptSha256 = $PackageReceiptSha256; fixture = $fixture; prerequisites = $prerequisites
    startedAtUtc = $started; completedAtUtc = [DateTime]::UtcNow.ToString("o"); result = $result
    failure = $failure; steps = $steps; assertions = $assertions
    limits = [ordered]@{ stepCount = 13; stepTimeoutMs = 600000; retainedOutputBytes = 262144 }
  }
  $json = $receipt | ConvertTo-Json -Depth 8
  if ([Text.Encoding]::UTF8.GetByteCount($json) -gt 262144) { throw "Installed smoke receipt exceeds its retention bound." }
  [IO.File]::WriteAllText($receiptPath, "$json`n", [Text.UTF8Encoding]::new($false))
  $json
}
