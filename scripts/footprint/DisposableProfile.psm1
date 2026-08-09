Set-StrictMode -Version Latest

$script:MarkerName = ".sage-disposable-profile-v1"
$script:MarkerBytes = [Text.Encoding]::ASCII.GetBytes("sage-disposable-profile-v1`n")
$script:MaximumNormalEntries = 4096
$script:MaximumNormalBytes = 512MB
$script:MaximumNormalPathBytes = 4MB

function Get-FootprintNormalizedPath {
  param([Parameter(Mandatory)][string]$Path)
  return [IO.Path]::GetFullPath($Path).TrimEnd([char[]]@('\', '/'))
}

function Get-FootprintStringSha256 {
  param([Parameter(Mandatory)][string]$Value)
  $hasher = [Security.Cryptography.SHA256]::Create()
  try {
    return ([BitConverter]::ToString($hasher.ComputeHash(
      [Text.Encoding]::UTF8.GetBytes($Value)
    ))).Replace("-", "").ToLowerInvariant()
  } finally {
    $hasher.Dispose()
  }
}

function Assert-C0BPlainLocalDirectory {
  param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$Label)
  if (-not [IO.Path]::IsPathRooted($Path) -or [IO.Path]::GetPathRoot($Path) -notmatch '^[A-Za-z]:\\$') {
    throw "$Label must be an absolute local drive path."
  }
  $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
  if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw "$Label must be a plain directory."
  }
  return $item
}

function Get-C0BProfileIdentitySha256 {
  param([Parameter(Mandatory)][string]$CanonicalRoot)
  $hasher = [Security.Cryptography.SHA256]::Create()
  try {
    $bytes = [Text.Encoding]::UTF8.GetBytes($CanonicalRoot.ToLowerInvariant())
    return ([BitConverter]::ToString($hasher.ComputeHash($bytes))).Replace("-", "").ToLowerInvariant()
  } finally {
    $hasher.Dispose()
  }
}

function New-C0BDisposableProfile {
  param(
    [Parameter(Mandatory)][string]$CampaignRoot,
    [Parameter(Mandatory)][ValidatePattern('^[a-z0-9][a-z0-9-]{0,63}$')][string]$ProfileId
  )
  $parent = Assert-C0BPlainLocalDirectory $CampaignRoot "Campaign root"
  $profileRoot = Join-Path $parent.FullName $ProfileId
  $claimPath = Join-Path $parent.FullName (".{0}.sage-profile-claim" -f $ProfileId)
  $claim = $null
  $ownsProfile = $false
  try {
    $claim = [IO.FileStream]::new(
      $claimPath,
      [IO.FileMode]::CreateNew,
      [IO.FileAccess]::Write,
      [IO.FileShare]::None,
      1,
      [IO.FileOptions]::WriteThrough
    )
    if (Test-Path -LiteralPath $profileRoot) {
      throw "Disposable profile '$ProfileId' already exists."
    }
    $created = [IO.Directory]::CreateDirectory($profileRoot)
    $ownsProfile = $true
    $plain = Assert-C0BPlainLocalDirectory $created.FullName "Disposable profile"
    $marker = Join-Path $plain.FullName $script:MarkerName
    $stream = [IO.FileStream]::new(
      $marker,
      [IO.FileMode]::CreateNew,
      [IO.FileAccess]::Write,
      [IO.FileShare]::None,
      4096,
      [IO.FileOptions]::WriteThrough
    )
    try {
      $stream.Write($script:MarkerBytes, 0, $script:MarkerBytes.Length)
      $stream.Flush($true)
    } finally {
      $stream.Dispose()
    }
    $entries = @(Get-ChildItem -LiteralPath $plain.FullName -Force)
    if ($entries.Count -ne 1 -or $entries[0].Name -ne $script:MarkerName -or
        ($entries[0].Attributes -band [IO.FileAttributes]::ReparsePoint) -or
        $entries[0].PSIsContainer) {
      throw "Disposable profile inventory is invalid."
    }
    $actualMarker = [IO.File]::ReadAllBytes($marker)
    if ($actualMarker.Length -ne $script:MarkerBytes.Length -or
        [Convert]::ToBase64String($actualMarker) -ne [Convert]::ToBase64String($script:MarkerBytes)) {
      throw "Disposable profile marker is invalid."
    }
    return [ordered]@{
      profileId = $ProfileId
      root = $plain.FullName
      identitySha256 = Get-C0BProfileIdentitySha256 $plain.FullName
    }
  } catch {
    if ($ownsProfile -and (Test-Path -LiteralPath $profileRoot)) {
      Remove-Item -LiteralPath $profileRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
    throw
  } finally {
    if ($null -ne $claim) {
      $claim.Dispose()
      Remove-Item -LiteralPath $claimPath -Force -ErrorAction SilentlyContinue
    }
  }
}

function Remove-C0BDisposableProfile {
  param(
    [Parameter(Mandatory)][string]$CampaignRoot,
    [Parameter(Mandatory)][string]$ProfileRoot
  )
  $parent = Assert-C0BPlainLocalDirectory $CampaignRoot "Campaign root"
  $profile = Assert-C0BPlainLocalDirectory $ProfileRoot "Disposable profile"
  if (-not [string]::Equals(
      [IO.Path]::GetFullPath([IO.Path]::GetDirectoryName($profile.FullName)),
      [IO.Path]::GetFullPath($parent.FullName),
      [StringComparison]::OrdinalIgnoreCase
    )) {
    throw "Disposable profile is not an immediate child of its campaign root."
  }
  $marker = Join-Path $profile.FullName $script:MarkerName
  if (-not (Test-Path -LiteralPath $marker -PathType Leaf)) {
    throw "Disposable profile marker is missing."
  }
  $actualMarker = [IO.File]::ReadAllBytes($marker)
  if ($actualMarker.Length -ne $script:MarkerBytes.Length -or
      [Convert]::ToBase64String($actualMarker) -ne [Convert]::ToBase64String($script:MarkerBytes)) {
    throw "Disposable profile marker is invalid."
  }
  Remove-Item -LiteralPath $profile.FullName -Recurse -Force -ErrorAction Stop
}

function Get-FootprintBoundedDirectoryInventory {
  param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$Label)
  $pathHash = Get-FootprintStringSha256 ((Get-FootprintNormalizedPath $Path).ToLowerInvariant())
  if (-not (Test-Path -LiteralPath $Path)) {
    return [ordered]@{ label = $Label; pathSha256 = $pathHash; exists = $false; entryCount = 0; totalFileBytes = 0; aggregateSha256 = Get-FootprintStringSha256 "absent" }
  }
  $root = (Assert-C0BPlainLocalDirectory $Path $Label).FullName
  $items = @(Get-ChildItem -LiteralPath $root -Force -Recurse | Select-Object -First ($script:MaximumNormalEntries + 1))
  if ($items.Count -gt $script:MaximumNormalEntries) { throw "$Label exceeds its entry limit." }
  $records = @()
  $totalBytes = [long]0
  $pathBytes = [long]0
  foreach ($item in $items) {
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "$Label contains a reparse point." }
    $relative = $item.FullName.Substring($root.Length).TrimStart([char[]]@('\', '/')).Replace('\', '/')
    $pathBytes += [Text.Encoding]::UTF8.GetByteCount($relative)
    if ($pathBytes -gt $script:MaximumNormalPathBytes) { throw "$Label exceeds its path-byte limit." }
    if ($item.PSIsContainer) { $records += "D`0$relative"; continue }
    $length = [long]$item.Length
    $totalBytes += $length
    if ($totalBytes -gt $script:MaximumNormalBytes) { throw "$Label exceeds its byte limit." }
    $hash = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($length -ne [long](Get-Item -LiteralPath $item.FullName).Length) { throw "$Label changed during inventory." }
    $records += "F`0$relative`0$length`0$hash"
  }
  return [ordered]@{
    label = $Label
    pathSha256 = $pathHash
    exists = $true
    entryCount = $items.Count
    totalFileBytes = $totalBytes
    aggregateSha256 = Get-FootprintStringSha256 (@($records | Sort-Object) -join "`n")
  }
}

function Get-FootprintNormalDataInventory {
  $roaming = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::ApplicationData)) "ai.sage.desktop"
  $local = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)) "ai.sage.desktop"
  $durable = Get-FootprintBoundedDirectoryInventory $roaming "roaming-durable-ai.sage.desktop"
  return [ordered]@{
    schemaVersion = 2
    durableRoaming = $durable
    durableAggregateSha256 = $durable.aggregateSha256
    disposableLocalCache = [ordered]@{
      label = "local-webview-cache-ai.sage.desktop"
      pathSha256 = Get-FootprintStringSha256 ((Get-FootprintNormalizedPath $local).ToLowerInvariant())
      existsAtCapture = Test-Path -LiteralPath $local -PathType Container
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
  foreach ($inventory in @($Before, $After)) {
    if ((($inventory.Keys | Sort-Object) -join "`0") -ne
        ((@("disposableLocalCache", "durableAggregateSha256", "durableRoaming", "schemaVersion") | Sort-Object) -join "`0") -or
        $inventory.schemaVersion -ne 2 -or
        (($inventory.durableRoaming.Keys | Sort-Object) -join "`0") -ne
        ((@("aggregateSha256", "entryCount", "exists", "label", "pathSha256", "totalFileBytes") | Sort-Object) -join "`0") -or
        (($inventory.disposableLocalCache.Keys | Sort-Object) -join "`0") -ne
        ((@("existsAtCapture", "expectedChurn", "label", "pathSha256", "policy") | Sort-Object) -join "`0") -or
        $inventory.durableRoaming.pathSha256 -notmatch '^[0-9a-f]{64}$' -or
        $inventory.durableRoaming.aggregateSha256 -notmatch '^[0-9a-f]{64}$' -or
        $inventory.durableAggregateSha256 -ne $inventory.durableRoaming.aggregateSha256 -or
        $inventory.disposableLocalCache.pathSha256 -notmatch '^[0-9a-f]{64}$' -or
        $inventory.disposableLocalCache.policy -ne "disposable-uncontrolled-not-an-isolation-gate") {
      throw "Normal-data inventory shape or identity is invalid."
    }
  }
  $durableFields = @("label", "pathSha256", "exists", "entryCount", "totalFileBytes", "aggregateSha256")
  if (@($durableFields | Where-Object { $Before.durableRoaming[$_] -ne $After.durableRoaming[$_] }).Count -ne 0 -or
      $Before.disposableLocalCache.label -ne $After.disposableLocalCache.label -or
      $Before.disposableLocalCache.pathSha256 -ne $After.disposableLocalCache.pathSha256 -or
      $Before.disposableLocalCache.policy -ne $After.disposableLocalCache.policy -or
      $Before.disposableLocalCache.expectedChurn -ne $After.disposableLocalCache.expectedChurn) {
    throw "Normal durable Roaming Sage state changed during an isolated launch."
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
  $actionFailure = $null
  try { $result = & $Action } catch { $actionFailure = $_ }
  try {
    $after = & $CaptureAfter
    [void](Assert-FootprintNormalDataUnchanged $Before $after)
  } catch {
    if ($null -ne $actionFailure) {
      throw "Action failed: $($actionFailure.Exception.Message) Normal-data isolation guard also failed: $($_.Exception.Message)"
    }
    throw
  }
  if ($null -ne $actionFailure) { throw $actionFailure }
  return [ordered]@{ result = $result; after = $after }
}

Export-ModuleMember -Function @(
  "Assert-FootprintNormalDataUnchanged",
  "Get-FootprintBoundedDirectoryInventory",
  "Get-FootprintNormalDataInventory",
  "Get-FootprintNormalizedPath",
  "Get-FootprintStringSha256",
  "Invoke-FootprintNormalDataGuardedAction",
  "New-C0BDisposableProfile",
  "Remove-C0BDisposableProfile"
)
