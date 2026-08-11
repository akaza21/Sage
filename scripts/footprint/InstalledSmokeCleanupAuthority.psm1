Set-StrictMode -Version Latest

function Get-InstalledSmokeCleanupAuthorityAuthentication {
  param(
    [Parameter(Mandatory)]$Value,
    [Parameter(Mandatory)][byte[]]$Key
  )
  if ($Key.Length -ne 32) { throw "Installed smoke cleanup authority key is invalid." }
  $uninstaller = $Value.uninstaller
  $bytesText = if ($null -eq $uninstaller) { "" } else { ([long]$uninstaller.bytes).ToString([Globalization.CultureInfo]::InvariantCulture) }
  $canonical = @(
    "schemaVersion=$($Value.schemaVersion)",
    "campaignId=$($Value.campaignId)",
    "state=$($Value.state)",
    "installRootLabel=$($Value.installRootLabel)",
    "relativePath=$(if ($null -eq $uninstaller) { '' } else { $uninstaller.relativePath })",
    "sha256=$(if ($null -eq $uninstaller) { '' } else { $uninstaller.sha256 })",
    "bytes=$bytesText",
    ""
  ) -join "`n"
  $hmac = [Security.Cryptography.HMACSHA256]::new($Key)
  try {
    return ([BitConverter]::ToString($hmac.ComputeHash([Text.Encoding]::UTF8.GetBytes($canonical)))).Replace("-", "").ToLowerInvariant()
  } finally { $hmac.Dispose() }
}

function Write-InstalledSmokeDurableRecord {
  param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory)]$Value,
    [ValidateRange(1, 32768)][int]$MaximumBytes = 16384
  )
  $json = $Value | ConvertTo-Json -Compress -Depth 6
  $bytes = [Text.Encoding]::UTF8.GetBytes("$json`n")
  if ($bytes.Length -gt $MaximumBytes) { throw "Installed smoke durable record exceeds its bound." }
  $temporary = "$Path.tmp"
  if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force }
  $stream = [IO.FileStream]::new(
    $temporary,
    [IO.FileMode]::CreateNew,
    [IO.FileAccess]::Write,
    [IO.FileShare]::None,
    4096,
    [IO.FileOptions]::WriteThrough
  )
  try {
    $stream.Write($bytes, 0, $bytes.Length)
    $stream.Flush($true)
  } finally { $stream.Dispose() }
  if (Test-Path -LiteralPath $Path -PathType Leaf) {
    [IO.File]::Replace($temporary, $Path, $null)
  } else {
    [IO.File]::Move($temporary, $Path)
  }
}

function Write-InstalledSmokeNewDurableBytes {
  param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory)][byte[]]$Bytes,
    [Parameter(Mandatory)][ValidateRange(1, 262144)][int]$MaximumBytes
  )
  if ($Bytes.Length -lt 1 -or $Bytes.Length -gt $MaximumBytes) {
    throw "Installed smoke authenticated output exceeds its bound."
  }
  $stream = [IO.FileStream]::new(
    $Path,
    [IO.FileMode]::CreateNew,
    [IO.FileAccess]::Write,
    [IO.FileShare]::None,
    4096,
    [IO.FileOptions]::WriteThrough
  )
  try {
    $stream.Write($Bytes, 0, $Bytes.Length)
    $stream.Flush($true)
  } finally { $stream.Dispose() }
}

function Write-InstalledSmokeAuthenticatedReceipt {
  param(
    [Parameter(Mandatory)][string]$ReceiptPath,
    [Parameter(Mandatory)][string]$AuthenticationPath,
    [Parameter(Mandatory)][string]$CampaignId,
    [Parameter(Mandatory)][byte[]]$ReceiptBytes,
    [Parameter(Mandatory)][byte[]]$Key
  )
  if ($Key.Length -ne 32) { throw "Installed smoke receipt key is invalid." }
  Write-InstalledSmokeNewDurableBytes $ReceiptPath $ReceiptBytes 262144
  $sha = [Security.Cryptography.SHA256]::Create()
  $hmac = [Security.Cryptography.HMACSHA256]::new($Key)
  try {
    $record = [ordered]@{
      schemaVersion = 1
      campaignId = $CampaignId
      receiptSha256 = ([BitConverter]::ToString($sha.ComputeHash($ReceiptBytes))).Replace("-", "").ToLowerInvariant()
      authenticationSha256 = ([BitConverter]::ToString($hmac.ComputeHash($ReceiptBytes))).Replace("-", "").ToLowerInvariant()
    }
  } finally {
    $sha.Dispose()
    $hmac.Dispose()
  }
  $authenticationBytes = [Text.Encoding]::UTF8.GetBytes("$(($record | ConvertTo-Json -Compress))`n")
  Write-InstalledSmokeNewDurableBytes $AuthenticationPath $authenticationBytes 1024
}

function Set-InstalledSmokeCleanupAuthority {
  param(
    [Parameter(Mandatory)]$Config,
    [Parameter(Mandatory)][ValidateSet("installing", "installed")][string]$State,
    $Uninstaller = $null
  )
  if ($State -eq "installed" -and $null -eq $Uninstaller) {
    throw "Installed cleanup authority requires an uninstaller identity."
  }
  $relativePath = if ($null -ne $Uninstaller) {
    $expected = [IO.Path]::GetFullPath((Join-Path $Config.outputRoot "installed\uninstall.exe"))
    if (-not [string]::Equals([IO.Path]::GetFullPath([string]$Uninstaller.path), $expected, [StringComparison]::OrdinalIgnoreCase)) {
      throw "Cleanup authority path is not canonical."
    }
    "installed/uninstall.exe"
  } else { $null }
  $record = [ordered]@{
    schemaVersion = 1
    campaignId = $Config.campaignId
    state = $State
    installRootLabel = "installed"
    uninstaller = if ($null -eq $Uninstaller) { $null } else { [ordered]@{
      relativePath = $relativePath
      sha256 = [string]$Uninstaller.sha256
      bytes = [long]$Uninstaller.bytes
    }}
    authenticationSha256 = $null
  }
  $record.authenticationSha256 = Get-InstalledSmokeCleanupAuthorityAuthentication $record $Config.cleanupKey
  Write-InstalledSmokeDurableRecord $Config.cleanupAuthorityPath $record
}

function Remove-InstalledSmokeCleanupAuthority {
  param([Parameter(Mandatory)]$Config)
  if (Test-Path -LiteralPath $Config.cleanupAuthorityPath -PathType Leaf) {
    Remove-Item -LiteralPath $Config.cleanupAuthorityPath -Force
  }
  $temporary = "$($Config.cleanupAuthorityPath).tmp"
  if (Test-Path -LiteralPath $temporary -PathType Leaf) {
    Remove-Item -LiteralPath $temporary -Force
  }
}

Export-ModuleMember -Function @(
  "Write-InstalledSmokeDurableRecord",
  "Write-InstalledSmokeAuthenticatedReceipt",
  "Get-InstalledSmokeCleanupAuthorityAuthentication",
  "Set-InstalledSmokeCleanupAuthority",
  "Remove-InstalledSmokeCleanupAuthority"
)
