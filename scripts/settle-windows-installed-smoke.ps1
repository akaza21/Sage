param(
  [Parameter(Mandatory)][ValidatePattern('^[a-z0-9][a-z0-9-]{0,63}$')][string]$CampaignId,
  [Parameter(Mandatory)][string]$OutputDirectory,
  [Parameter(Mandatory)][string]$AuthenticatedSourceRoot,
  [Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9+/]{43}=$')][string]$CleanupKeyBase64,
  [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{64}$')][string]$AuthoritySha256,
  [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{64}$')][string]$PowerShellSha256,
  [Parameter(Mandatory)][ValidateRange(1, 536870912)][long]$PowerShellBytes,
  [Parameter(Mandatory)][ValidateLength(1, 32768)][string]$RunnerFileIdentitiesBase64
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$runnerRelativePaths = @(
  "scripts/settle-windows-installed-smoke.ps1",
  "scripts/footprint/InstalledSmokeCleanupAuthority.psm1",
  "scripts/footprint/InstalledSmokeIdentity.psm1",
  "scripts/footprint/InstalledSmokePackageLifecycle.psm1",
  "scripts/footprint/WindowsJob.psm1",
  "scripts/footprint/WindowsJobCompletion.psm1"
)
$sourceRoot = [IO.Path]::GetFullPath($AuthenticatedSourceRoot)
[object[]]$runnerIdentities = ([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($RunnerFileIdentitiesBase64)) | ConvertFrom-Json)

function Test-SettlementFileIdentity {
  param([string]$Path, [long]$Bytes, [string]$Sha256)
  try {
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    return -not $item.PSIsContainer -and
      -not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -and
      $item.Length -eq $Bytes -and
      (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() -eq $Sha256
  } catch { return $false }
}

function Test-SettlementFileUnderRoot {
  param([string]$Path, [string]$Root, [long]$Bytes, [string]$Sha256)
  try {
    $rootPath = [IO.Path]::GetFullPath($Root).TrimEnd('\')
    $fullPath = [IO.Path]::GetFullPath($Path)
    $volumeRoot = [IO.Path]::GetPathRoot($rootPath)
    if ([string]::IsNullOrWhiteSpace($volumeRoot) -or
        [string]::Equals($rootPath, $volumeRoot.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase) -or
        -not $fullPath.StartsWith("$rootPath\", [StringComparison]::OrdinalIgnoreCase)) { return $false }
    $current = $volumeRoot
    foreach ($segment in $fullPath.Substring($volumeRoot.Length).Split('\')) {
      $current = Join-Path $current $segment
      $item = Get-Item -LiteralPath $current -Force -ErrorAction Stop
      if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { return $false }
    }
    return Test-SettlementFileIdentity $fullPath $Bytes $Sha256
  } catch { return $false }
}

function Assert-SettlementRunnerIdentity {
  $hostPath = (Get-Process -Id $PID -ErrorAction Stop).Path
  if (-not (Test-SettlementFileIdentity $hostPath $PowerShellBytes $PowerShellSha256)) {
    throw "Fallback PowerShell identity changed."
  }
  if ($runnerIdentities.Count -ne $runnerRelativePaths.Count) { throw "Fallback runner closure is incomplete." }
  for ($index = 0; $index -lt $runnerRelativePaths.Count; $index += 1) {
    $expected = $runnerIdentities[$index]
    if ([string]$expected.relativePath -ne $runnerRelativePaths[$index] -or
        [long]$expected.bytes -lt 1 -or [long]$expected.bytes -gt 2097152 -or
        [string]$expected.sha256 -notmatch '^[0-9a-f]{64}$' -or
        -not (Test-SettlementFileUnderRoot (Join-Path $sourceRoot $expected.relativePath) $sourceRoot ([long]$expected.bytes) ([string]$expected.sha256))) {
      throw "Fallback runner source identity changed."
    }
  }
}

function Assert-SettlementExactKeys {
  param($Value, [string[]]$Keys, [string]$Label)
  $observed = @($Value.PSObject.Properties.Name | Sort-Object)
  $expected = @($Keys | Sort-Object)
  if ($observed.Count -ne $expected.Count -or [string]::Join("`0", $observed) -ne [string]::Join("`0", $expected)) {
    throw "$Label shape is invalid."
  }
}

Assert-SettlementRunnerIdentity
Import-Module (Join-Path $sourceRoot "scripts/footprint/InstalledSmokeCleanupAuthority.psm1") -Force
Import-Module (Join-Path $sourceRoot "scripts/footprint/InstalledSmokeIdentity.psm1") -Force
Import-Module (Join-Path $sourceRoot "scripts/footprint/WindowsJob.psm1") -Force
Import-Module (Join-Path $sourceRoot "scripts/footprint/WindowsJobCompletion.psm1") -Force
Import-Module (Join-Path $sourceRoot "scripts/footprint/InstalledSmokePackageLifecycle.psm1") -Force
[byte[]]$cleanupKey = [Convert]::FromBase64String($CleanupKeyBase64)
if ($cleanupKey.Length -ne 32) { throw "Fallback cleanup authority key is invalid." }

$resolvedOutput = Assert-InstalledSmokePath ([IO.Path]::GetFullPath($OutputDirectory)) ([IO.Path]::GetFullPath($OutputDirectory)) "Directory"
$authorityPath = Join-Path $resolvedOutput "cleanup-authority.json"
$authority = Get-InstalledSmokeFileIdentity $authorityPath $resolvedOutput
if ($authority.bytes -gt 16384 -or $authority.sha256 -ne $AuthoritySha256) { throw "Cleanup authority identity changed." }
$record = Get-Content -LiteralPath $authorityPath -Raw | ConvertFrom-Json
Assert-SettlementExactKeys $record @("schemaVersion", "campaignId", "state", "installRootLabel", "uninstaller", "authenticationSha256") "Cleanup authority"
Assert-SettlementExactKeys $record.uninstaller @("relativePath", "sha256", "bytes") "Cleanup uninstaller"
if ([int]$record.schemaVersion -ne 1 -or [string]$record.campaignId -ne $CampaignId -or
    [string]$record.state -ne "installed" -or [string]$record.installRootLabel -ne "installed" -or
    [string]$record.uninstaller.relativePath -ne "installed/uninstall.exe" -or
    [string]$record.uninstaller.sha256 -notmatch '^[0-9a-f]{64}$' -or
    [long]$record.uninstaller.bytes -lt 1 -or [long]$record.uninstaller.bytes -gt 536870912 -or
    [string]$record.authenticationSha256 -notmatch '^[0-9a-f]{64}$' -or
    (Get-InstalledSmokeCleanupAuthorityAuthentication $record $cleanupKey) -ne [string]$record.authenticationSha256) {
  throw "Cleanup authority value is invalid."
}
$installRoot = Join-Path $resolvedOutput "installed"
$uninstallerPath = Join-Path $installRoot "uninstall.exe"
[void](Assert-InstalledSmokeFileIdentity $uninstallerPath $resolvedOutput ([long]$record.uninstaller.bytes) ([string]$record.uninstaller.sha256))
$config = [ordered]@{
  outputRoot = $resolvedOutput; installRoot = $installRoot
  campaignId = $CampaignId; cleanupAuthorityPath = $authorityPath
  cleanupKey = $cleanupKey
}
$state = [ordered]@{
  stepIds = @("uninstall-final")
  steps = @([ordered]@{ id = "uninstall-final"; status = "skipped"; durationMs = 0; evidenceSha256 = $null })
  currentStep = "uninstall-final"; installed = $true
  cleanupUninstaller = [ordered]@{ path = $uninstallerPath; sha256 = [string]$record.uninstaller.sha256; bytes = [long]$record.uninstaller.bytes }
}
$identityGate = { Assert-SettlementRunnerIdentity }
$evidence = Invoke-InstalledSmokeUninstall $state $identityGate "uninstall-final" $state.cleanupUninstaller $config
$result = [ordered]@{
  schemaVersion = 1; campaignId = $CampaignId; status = "uninstalled"
  authoritySha256 = $AuthoritySha256; jobConverged = [bool]$evidence.jobConverged
}
Write-InstalledSmokeDurableRecord (Join-Path $resolvedOutput "fallback-cleanup.json") $result
