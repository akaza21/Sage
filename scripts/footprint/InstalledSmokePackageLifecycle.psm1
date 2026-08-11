Set-StrictMode -Version Latest

function Get-InstalledSmokeLifecycleSha256 {
  param([Parameter(Mandatory)][string]$Value)
  $hasher = [Security.Cryptography.SHA256]::Create()
  try {
    return ([BitConverter]::ToString($hasher.ComputeHash([Text.Encoding]::UTF8.GetBytes($Value)))).Replace("-", "").ToLowerInvariant()
  } finally { $hasher.Dispose() }
}

function Set-InstalledSmokeLifecycleStep {
  param($State, [string]$Id, [Diagnostics.Stopwatch]$Timer, $Evidence)
  $index = [Array]::IndexOf([string[]]$State.stepIds, $Id)
  $State.steps[$index] = [ordered]@{
    id = $Id; status = "passed"
    durationMs = [Math]::Min(600000, [Math]::Max(0, [int]$Timer.ElapsedMilliseconds))
    evidenceSha256 = Get-InstalledSmokeLifecycleSha256 ($Evidence | ConvertTo-Json -Compress -Depth 6)
  }
}

function Invoke-InstalledSmokeJobProcess {
  param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string[]]$Arguments,
    [ValidateRange(1, 120)][int]$TimeoutSeconds = 120
  )
  $start = [Diagnostics.ProcessStartInfo]::new()
  $start.FileName = $Executable
  $start.WorkingDirectory = [IO.Path]::GetDirectoryName($Executable)
  $start.UseShellExecute = $false
  $start.CreateNoWindow = $true
  $launch = Start-FootprintJobProcess $start -ArgumentList $Arguments -ActiveProcessLimit 33
  try {
    $evidence = Wait-FootprintJobProcess $launch.process $launch.boundary ($TimeoutSeconds * 1000) 500
  } finally { $launch.boundary.Dispose() }
  if ($evidence.exitCode -ne 0) { throw "Installer process exited with code $($evidence.exitCode)." }
  return [ordered]@{
    exitCode = $evidence.exitCode
    zeroOrphans = $evidence.orphanProcessesAfterClose -eq 0
  }
}

function Assert-InstalledSmokeInstallerIdentity {
  param($Config, [scriptblock]$IdentityGate)
  & $IdentityGate
  $item = Get-Item -LiteralPath $Config.installerPath -Force -ErrorAction Stop
  if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
      $item.Length -ne $Config.installerBytes -or
      (Get-FileHash -LiteralPath $Config.installerPath -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Config.installerSha256) {
    throw "Authenticated installer identity changed before install."
  }
  $signature = Get-AuthenticodeSignature -LiteralPath $Config.installerPath
  if ($signature.Status -ne [Management.Automation.SignatureStatus]::Valid -or
      $null -eq $signature.SignerCertificate -or
      $signature.SignerCertificate.Thumbprint.ToLowerInvariant() -ne $Config.signerThumbprint.ToLowerInvariant()) {
    throw "Authenticated installer signer changed before install."
  }
}

function Invoke-InstalledSmokeInstall {
  param($State, [scriptblock]$IdentityGate, [string]$StepId, $Config)
  $State.currentStep = $StepId
  $timer = [Diagnostics.Stopwatch]::StartNew()
  Assert-InstalledSmokeInstallerIdentity $Config $IdentityGate
  [void](Assert-InstalledSmokePath $Config.installRoot $Config.outputRoot "Any" -AllowMissingLeaf)
  if (Test-Path -LiteralPath $Config.installRoot) { throw "Install root must be absent before install." }
  Set-InstalledSmokeCleanupAuthority $Config "installing"
  $State.installed = $true
  $processEvidence = Invoke-InstalledSmokeJobProcess $Config.installerPath @("/S", "/D=$($Config.installRoot)")
  $executable = Join-Path $Config.installRoot "sage.exe"
  $uninstaller = Join-Path $Config.installRoot "uninstall.exe"
  $State.cleanupUninstaller = Get-InstalledSmokeFileIdentity $uninstaller $Config.outputRoot
  Set-InstalledSmokeCleanupAuthority $Config "installed" $State.cleanupUninstaller
  $executableIdentity = Get-InstalledSmokeFileIdentity $executable $Config.outputRoot
  if ($executableIdentity.bytes -ne $Config.executableBytes -or
      $executableIdentity.sha256 -ne $Config.executableSha256) {
    throw "Installed executable does not match the authenticated package projection."
  }
  Set-InstalledSmokeLifecycleStep $State $StepId $timer ([ordered]@{
    executableSha256 = $executableIdentity.sha256; executableBytes = $executableIdentity.bytes
    uninstallerSha256 = $State.cleanupUninstaller.sha256; uninstallerBytes = $State.cleanupUninstaller.bytes
    zeroOrphans = $processEvidence.zeroOrphans
  })
  return [ordered]@{ executable = $executableIdentity; uninstaller = $State.cleanupUninstaller }
}

function Invoke-InstalledSmokeUninstall {
  param($State, [scriptblock]$IdentityGate, [string]$StepId, $Uninstaller, $Config)
  $State.currentStep = $StepId
  $timer = [Diagnostics.Stopwatch]::StartNew()
  & $IdentityGate
  $identity = Assert-InstalledSmokeFileIdentity $Uninstaller.path $Config.outputRoot ([long]$Uninstaller.bytes) ([string]$Uninstaller.sha256)
  $processEvidence = Invoke-InstalledSmokeJobProcess $identity.path @("/S")
  if (Test-Path -LiteralPath (Join-Path $Config.installRoot "sage.exe")) {
    throw "Uninstall retained the installed executable."
  }
  if (Test-Path -LiteralPath $Config.installRoot -PathType Container) {
    [void](Assert-InstalledSmokePath $Config.installRoot $Config.outputRoot "Directory")
    if (Test-Path -LiteralPath $identity.path -PathType Leaf) {
      [void](Assert-InstalledSmokeFileIdentity $identity.path $Config.outputRoot ([long]$identity.bytes) ([string]$identity.sha256))
      Remove-Item -LiteralPath $identity.path -Force
    }
    if (@(Get-ChildItem -LiteralPath $Config.installRoot -Force | Select-Object -First 1).Count -ne 0) {
      throw "Uninstall retained installed payload entries."
    }
    Remove-Item -LiteralPath $Config.installRoot -Force
  }
  $State.installed = $false
  $State.cleanupUninstaller = $null
  Remove-InstalledSmokeCleanupAuthority $Config
  Set-InstalledSmokeLifecycleStep $State $StepId $timer ([ordered]@{
    executableAbsent = $true; zeroOrphans = $processEvidence.zeroOrphans
    uninstallerSha256 = $identity.sha256; uninstallerBytes = $identity.bytes
  })
  return [ordered]@{ jobConverged = [bool]$processEvidence.zeroOrphans }
}

Export-ModuleMember -Function @(
  "Invoke-InstalledSmokeInstall",
  "Invoke-InstalledSmokeUninstall"
)
