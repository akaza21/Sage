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
  [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{64}$')][string]$NodeSha256,
  [Parameter(Mandatory)][ValidateRange(1, 536870912)][long]$NodeBytes,
  [Parameter(Mandatory)][string]$GitPath,
  [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{64}$')][string]$GitSha256,
  [Parameter(Mandatory)][ValidateRange(1, 536870912)][long]$GitBytes,
  [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{64}$')][string]$PowerShellSha256,
  [Parameter(Mandatory)][ValidateRange(1, 536870912)][long]$PowerShellBytes,
  [Parameter(Mandatory)][ValidateLength(1, 32768)][string]$RunnerFileIdentitiesBase64
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

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
$runnerRelativePaths = @(
  "scripts/measure-windows-installed-smoke.ps1",
  "scripts/footprint/InstalledSmokeIdentity.psm1",
  "scripts/footprint/InstalledSmokePackageLifecycle.psm1",
  "scripts/footprint/InstalledSmokeSemantic.psm1",
  "scripts/footprint/DisposableProfile.psm1",
  "scripts/footprint/ScenarioEvidence.psm1",
  "scripts/footprint/WindowsFootprint.psm1",
  "scripts/footprint/WindowsJob.psm1",
  "scripts/footprint/WindowsJobCompletion.psm1",
  "scripts/footprint/ProcessIdentity.psm1",
  "scripts/footprint/bounded-node-runner.mjs",
  "scripts/footprint/drive-installed-smoke.mjs"
)
$steps = @($stepIds | ForEach-Object {
  [ordered]@{ id = $_; status = "skipped"; durationMs = 0; evidenceSha256 = $null }
})
$assertions = [ordered]@{}
foreach ($name in $assertionNames) { $assertions[$name] = $false }
$runState = [ordered]@{
  stepIds = $stepIds; steps = $steps
  assertionNames = $assertionNames; assertions = $assertions
  currentStep = "prerequisites"; job = $null
  installed = $false; cleanupUninstaller = $null
}
$currentStep = "prerequisites"
$failure = $null
$result = "failed"
$started = [DateTime]::UtcNow.ToString("yyyy-MM-dd'T'HH:mm:ss.fff'Z'", [Globalization.CultureInfo]::InvariantCulture)
$profile = $null
$job = $null
$runnerIdentities = $null
$runner = $null
$driver = $null
$resolvedInstaller = $null
$resolvedNode = $null
$resolvedGit = $null

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

function Test-PlainFileUnderRoot {
  param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory)][string]$Root,
    [Parameter(Mandatory)][long]$Bytes,
    [Parameter(Mandatory)][string]$Sha256
  )
  try {
    $rootPath = [IO.Path]::GetFullPath($Root).TrimEnd('\')
    $fullPath = [IO.Path]::GetFullPath($Path)
    $volumeRoot = [IO.Path]::GetPathRoot($rootPath)
    if ([string]::IsNullOrWhiteSpace($volumeRoot) -or
        [string]::Equals($rootPath, $volumeRoot.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase) -or
        -not [string]::Equals([IO.Path]::GetPathRoot($fullPath), $volumeRoot, [StringComparison]::OrdinalIgnoreCase)) {
      return $false
    }
    $prefix = "$rootPath\"
    if (-not $fullPath.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { return $false }
    $current = $volumeRoot
    foreach ($segment in $fullPath.Substring($volumeRoot.Length).Split('\')) {
      $current = Join-Path $current $segment
      $item = Get-Item -LiteralPath $current -Force -ErrorAction Stop
      if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { return $false }
    }
    return Test-FileIdentity $fullPath $Bytes $Sha256
  } catch { return $false }
}

function Assert-PowerShellIdentity {
  $hostPath = (Get-Process -Id $PID -ErrorAction Stop).Path
  if (-not (Test-FileIdentity $hostPath $PowerShellBytes $PowerShellSha256)) {
    throw "PowerShell host identity changed during smoke execution."
  }
}

function Assert-NodeIdentity {
  if (-not (Test-FileIdentity $resolvedNode $NodeBytes $NodeSha256)) {
    throw "Authenticated Node identity changed during smoke execution."
  }
}

function Assert-GitIdentity {
  if (-not (Test-FileIdentity $resolvedGit $GitBytes $GitSha256)) {
    throw "Authenticated Git identity changed during smoke execution."
  }
}

function Assert-RunnerSourceIdentity {
  if ($null -eq $runnerIdentities -or $runnerIdentities.Count -ne $runnerRelativePaths.Count) {
    throw "Installed smoke runner identity set is incomplete."
  }
  for ($index = 0; $index -lt $runnerRelativePaths.Count; $index += 1) {
    $expected = $runnerIdentities[$index]
    if ([string]$expected.relativePath -ne $runnerRelativePaths[$index] -or
        [long]$expected.bytes -lt 1 -or [long]$expected.bytes -gt 2097152 -or
        [string]$expected.sha256 -notmatch '^[0-9a-f]{64}$' -or
        -not (Test-PlainFileUnderRoot (Join-Path $sourceRoot $expected.relativePath) $sourceRoot ([long]$expected.bytes) ([string]$expected.sha256))) {
      throw "Installed smoke runner source identity changed."
    }
  }
}

function Invoke-Bounded {
  param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string[]]$Arguments,
    [Parameter(Mandatory)][string]$Label,
    [ValidateRange(100, 120000)][int]$TimeoutMs = 120000
  )
  Assert-PowerShellIdentity
  Assert-NodeIdentity
  Assert-RunnerSourceIdentity
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

function Set-StepFailed {
  param([Parameter(Mandatory)][string]$Id)
  $index = [Array]::IndexOf($stepIds, $Id)
  if ($index -ge 0) {
    $steps[$index] = [ordered]@{ id = $Id; status = "failed"; durationMs = 0; evidenceSha256 = $null }
  }
}

if ($env:OS -ne "Windows_NT") { throw "This smoke harness requires Windows." }
$parent = Split-Path -Parent ([IO.Path]::GetFullPath($OutputDirectory))
if (-not (Test-Path -LiteralPath $parent -PathType Container)) { throw "Smoke output parent is missing." }
[void](Test-PlainDirectory $parent "Smoke output parent")
if (-not (Test-Path -LiteralPath $OutputDirectory -PathType Container)) { throw "Smoke output must be reserved." }
$resolvedOutput = Test-PlainDirectory ([IO.Path]::GetFullPath($OutputDirectory)) "Smoke output"
if (@(Get-ChildItem -LiteralPath $resolvedOutput -Force).Count -ne 0) { throw "Smoke output must be empty." }
$script:processOrdinal = 0
$sourceRoot = Split-Path -Parent $PSScriptRoot
$installRoot = Join-Path $resolvedOutput "installed"
$profileCampaignRoot = Join-Path $resolvedOutput "profiles"
$rootLabel = "sage-installed-smoke-fixture"
$fixtureRoot = Join-Path $resolvedOutput $rootLabel
$receiptPath = Join-Path $resolvedOutput "receipt.json"
$prerequisites = [ordered]@{
  windows = $false; interactiveSession = $false; webView2 = $false
  administrator = $false; authenticode = $false; git = $false
  osVersion = $null; webView2Version = $null; gitVersion = $null; signerThumbprint = $null
}
$fixture = [ordered]@{ rootIdentitySha256 = "0" * 64; rootLabel = $rootLabel; initialCommitSha256 = "0" * 64 }

try {
  try {
    $resolvedInstaller = (Resolve-Path -LiteralPath $InstallerPath).Path
    $resolvedNode = (Resolve-Path -LiteralPath $NodePath).Path
    $resolvedGit = (Resolve-Path -LiteralPath $GitPath).Path
    [object[]]$runnerIdentities = ([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($RunnerFileIdentitiesBase64)) | ConvertFrom-Json)
    Assert-PowerShellIdentity
    Assert-NodeIdentity
    Assert-GitIdentity
    Assert-RunnerSourceIdentity
    Import-Module (Join-Path $PSScriptRoot "footprint/InstalledSmokeIdentity.psm1") -Force
    Import-Module (Join-Path $PSScriptRoot "footprint/InstalledSmokePackageLifecycle.psm1") -Force
    Import-Module (Join-Path $PSScriptRoot "footprint/InstalledSmokeSemantic.psm1") -Force
    Import-Module (Join-Path $PSScriptRoot "footprint/DisposableProfile.psm1") -Force
    Import-Module (Join-Path $PSScriptRoot "footprint/ScenarioEvidence.psm1") -Force
    Import-Module (Join-Path $PSScriptRoot "footprint/WindowsFootprint.psm1") -Force
    Import-Module (Join-Path $PSScriptRoot "footprint/WindowsJob.psm1") -Force
    Import-Module (Join-Path $PSScriptRoot "footprint/WindowsJobCompletion.psm1") -Force
    $runner = (Resolve-Path (Join-Path $PSScriptRoot "footprint/bounded-node-runner.mjs")).Path
    $driver = (Resolve-Path (Join-Path $PSScriptRoot "footprint/drive-installed-smoke.mjs")).Path
    [void](Assert-InstalledSmokePath $resolvedOutput $resolvedOutput "Directory")
    $signature = Get-AuthenticodeSignature -LiteralPath $resolvedInstaller
    $webViewVersion = @(
      @(
        Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F1E7E89C-98C0-4E8A-8B72-6AD3D4C13C4B}" -ErrorAction SilentlyContinue
        Get-ItemProperty "HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F1E7E89C-98C0-4E8A-8B72-6AD3D4C13C4B}" -ErrorAction SilentlyContinue
      ) | Where-Object { $_.pv } | Select-Object -First 1 -ExpandProperty pv
    )
    $administrator = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    $gitVersion = (Invoke-Bounded $resolvedGit @("--version") "git-version" 30000).stdout.Trim().Replace("git version ", "")
    $prerequisites = [ordered]@{
      windows = $true
      interactiveSession = [Environment]::UserInteractive -and $env:SESSIONNAME -notmatch '^Services$'
      webView2 = $webViewVersion.Count -eq 1
      administrator = $administrator
      authenticode = (Test-FileIdentity $resolvedInstaller $InstallerBytes $InstallerSha256) -and
        $signature.Status -eq [Management.Automation.SignatureStatus]::Valid -and
        $null -ne $signature.SignerCertificate -and
        $signature.SignerCertificate.Thumbprint.ToLowerInvariant() -eq $ExpectedSignerThumbprint.ToLowerInvariant()
      git = -not [string]::IsNullOrWhiteSpace($gitVersion)
      osVersion = [Environment]::OSVersion.Version.ToString()
      webView2Version = if ($webViewVersion.Count -eq 1) { [string]$webViewVersion[0] } else { $null }
      gitVersion = $gitVersion
      signerThumbprint = if ($null -ne $signature.SignerCertificate) { $signature.SignerCertificate.Thumbprint.ToLowerInvariant() } else { $null }
    }
  } catch {
    $failure = [ordered]@{ code = "prerequisite-inspection-failed"; phase = "prerequisites" }
  }
  $missing = if ($null -eq $failure) { @(
    @("windows", "windows-unavailable"), @("interactiveSession", "interactive-session-unavailable"),
    @("webView2", "webview2-unavailable"), @("administrator", "administrator-unavailable"),
    @("authenticode", "installer-signature-invalid"), @("git", "git-unavailable")
  ) | Where-Object { -not [bool]$prerequisites[$_[0]] } | Select-Object -First 1 } else { $null }
  if ($null -ne $failure) {
    # Typed prerequisite inspection failure is settled in the final receipt.
  } elseif ($null -ne $missing) {
    $result = "blocked"
    $failure = [ordered]@{ code = $missing[1]; phase = "prerequisites" }
  } else {
    [void][IO.Directory]::CreateDirectory($profileCampaignRoot)
    [void](Assert-InstalledSmokePath $profileCampaignRoot $resolvedOutput "Directory")
    $fixture = New-InstalledSmokeFixtureRepository $fixtureRoot $rootLabel {
      param([string[]]$Arguments)
      Invoke-Bounded $resolvedGit $Arguments "git-fixture" 30000
    }
    $profile = New-C0BDisposableProfile $profileCampaignRoot "installed-smoke"
    $semanticConfig = [ordered]@{
      outputRoot = $resolvedOutput; installRoot = $installRoot
      profileRoot = $profile.root; fixtureRoot = $fixtureRoot
      gitPath = $resolvedGit; nodePath = $resolvedNode; driver = $driver
      rootLabel = $rootLabel
      invokeBounded = {
        param($Executable, [string[]]$Arguments, $Label, $TimeoutMs)
        Invoke-Bounded $Executable $Arguments $Label $TimeoutMs
      }
    }
    $identityGate = {
      Assert-PowerShellIdentity
      Assert-NodeIdentity
      Assert-GitIdentity
      Assert-RunnerSourceIdentity
    }
    $lifecycleConfig = [ordered]@{
      outputRoot = $resolvedOutput; installRoot = $installRoot
      installerPath = $resolvedInstaller; installerSha256 = $InstallerSha256
      installerBytes = $InstallerBytes; signerThumbprint = $ExpectedSignerThumbprint
      executableSha256 = $ExpectedInstalledExecutableSha256
      executableBytes = $ExpectedInstalledExecutableBytes
    }
    $payload = Invoke-InstalledSmokeInstall $runState $identityGate "install-initial" $lifecycleConfig
    Invoke-InstalledSmokeSemanticCampaign $runState $identityGate "launch-initial" "drive-primary" "close-initial" "primary" $payload.executable $semanticConfig
    Invoke-InstalledSmokeSemanticCampaign $runState $identityGate "launch-restart" "verify-restart" "close-restart" "restart" $payload.executable $semanticConfig
    Invoke-InstalledSmokeUninstall $runState $identityGate "uninstall-initial" $payload.uninstaller $lifecycleConfig
    $payload = Invoke-InstalledSmokeInstall $runState $identityGate "install-reinstall" $lifecycleConfig
    Invoke-InstalledSmokeSemanticCampaign $runState $identityGate "launch-reinstall" "verify-reinstall" "close-reinstall" "reinstall" $payload.executable $semanticConfig
    Invoke-InstalledSmokeUninstall $runState $identityGate "uninstall-final" $payload.uninstaller $lifecycleConfig
    if (@($assertions.Values | Where-Object { -not $_ }).Count -ne 0) { throw "Installed smoke assertions are incomplete." }
    $result = "passed"
  }
} catch {
  $currentStep = $runState.currentStep
  $job = $runState.job
  Set-StepFailed $currentStep
  $code = if ($_.Exception.Message -match 'timeout|exceeded') { "step-timeout" } elseif ($_.Exception.Message -match 'identity|orphan|cleanup') { "identity-lost" } else { "step-failed" }
  $failure = [ordered]@{ code = $code; phase = $currentStep }
} finally {
  if ($null -ne $job) {
    try {
      [void](Stop-FootprintJobProcess $job.process $job.boundary 0 5000)
      $job.boundary.Dispose()
      $job = $null
      $runState.job = $null
    } catch {
      try { $job.boundary.Terminate() } catch {}
      try { $job.boundary.Dispose() } catch {}
      $job = $null
      $runState.job = $null
      $failure = [ordered]@{ code = "cleanup-failed"; phase = "job-cleanup" }
    }
  }
  if ($runState.installed) {
    try {
      if ($null -eq $runState.cleanupUninstaller) { throw "Partial install has no authoritative uninstaller." }
      Invoke-InstalledSmokeUninstall $runState $identityGate "uninstall-final" $runState.cleanupUninstaller $lifecycleConfig
    } catch {
      Set-StepFailed "uninstall-final"
      $failure = [ordered]@{ code = "cleanup-failed"; phase = "uninstall-final" }
    }
  }
  if ($null -ne $profile) {
    try { Remove-C0BDisposableProfile $profileCampaignRoot $profile.root } catch { $failure = [ordered]@{ code = "cleanup-failed"; phase = "profile-cleanup" } }
  }
  if (Test-Path -LiteralPath $fixtureRoot -PathType Container) {
    try {
      [void](Get-FootprintBoundedDirectoryInventory $fixtureRoot "Installed smoke fixture")
      [void](Assert-InstalledSmokePath $fixtureRoot $resolvedOutput "Directory")
      Remove-Item -LiteralPath $fixtureRoot -Recurse -Force
    } catch { $failure = [ordered]@{ code = "cleanup-failed"; phase = "fixture-cleanup" } }
  }
  if (Test-Path -LiteralPath $profileCampaignRoot -PathType Container) {
    try {
      [void](Assert-InstalledSmokePath $profileCampaignRoot $resolvedOutput "Directory")
      if (@(Get-ChildItem -LiteralPath $profileCampaignRoot -Force | Select-Object -First 1).Count -ne 0) {
        throw "Disposable profile campaign root is not empty."
      }
      Remove-Item -LiteralPath $profileCampaignRoot -Force
    } catch { $failure = [ordered]@{ code = "cleanup-failed"; phase = "profile-cleanup" } }
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
    startedAtUtc = $started
    completedAtUtc = [DateTime]::UtcNow.ToString("yyyy-MM-dd'T'HH:mm:ss.fff'Z'", [Globalization.CultureInfo]::InvariantCulture)
    result = $result
    failure = $failure; steps = $steps; assertions = $assertions
    limits = [ordered]@{ stepCount = 13; stepTimeoutMs = 600000; retainedOutputBytes = 262144 }
  }
  $json = $receipt | ConvertTo-Json -Depth 8
  if ([Text.Encoding]::UTF8.GetByteCount($json) -gt 262144) { throw "Installed smoke receipt exceeds its retention bound." }
  [IO.File]::WriteAllText($receiptPath, "$json`n", [Text.UTF8Encoding]::new($false))
  $json
}
