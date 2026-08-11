Set-StrictMode -Version Latest

function Assert-InstalledSmokePath {
  param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory)][string]$Root,
    [ValidateSet("Any", "File", "Directory")][string]$LeafKind = "Any",
    [switch]$AllowMissingLeaf
  )
  $rootPath = [IO.Path]::GetFullPath($Root).TrimEnd('\')
  $fullPath = [IO.Path]::GetFullPath($Path)
  $volumeRoot = [IO.Path]::GetPathRoot($rootPath)
  if ([string]::IsNullOrWhiteSpace($volumeRoot) -or
      [string]::Equals($rootPath, $volumeRoot.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase) -or
      -not [string]::Equals([IO.Path]::GetPathRoot($fullPath), $volumeRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Installed smoke reserved root is ambiguous."
  }
  $prefix = "$rootPath\"
  if (-not [string]::Equals($fullPath, $rootPath, [StringComparison]::OrdinalIgnoreCase) -and
      -not $fullPath.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Installed smoke path escaped its reserved root."
  }
  $relative = $fullPath.Substring($volumeRoot.Length)
  $segments = @($relative.Split(@('\'), [StringSplitOptions]::RemoveEmptyEntries))
  $current = $volumeRoot
  for ($index = 0; $index -lt $segments.Count; $index += 1) {
    $current = Join-Path $current $segments[$index]
    $last = $index -eq $segments.Count - 1
    if (-not (Test-Path -LiteralPath $current)) {
      if ($last -and $AllowMissingLeaf) { continue }
      throw "Installed smoke path component is missing."
    }
    $item = Get-Item -LiteralPath $current -Force -ErrorAction Stop
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
      throw "Installed smoke path contains a reparse point."
    }
    if (-not $last -and -not $item.PSIsContainer) {
      throw "Installed smoke path ancestor is not a directory."
    }
    if ($last -and $LeafKind -eq "File" -and $item.PSIsContainer) {
      throw "Installed smoke path leaf is not a file."
    }
    if ($last -and $LeafKind -eq "Directory" -and -not $item.PSIsContainer) {
      throw "Installed smoke path leaf is not a directory."
    }
  }
  return $fullPath
}

function Get-InstalledSmokeFileIdentity {
  param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory)][string]$Root
  )
  $resolved = Assert-InstalledSmokePath $Path $Root "File"
  $item = Get-Item -LiteralPath $resolved -Force -ErrorAction Stop
  return [ordered]@{
    path = $resolved
    bytes = [long]$item.Length
    sha256 = (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash.ToLowerInvariant()
  }
}

function Assert-InstalledSmokeFileIdentity {
  param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory)][string]$Root,
    [Parameter(Mandatory)][long]$Bytes,
    [Parameter(Mandatory)][string]$Sha256
  )
  $identity = Get-InstalledSmokeFileIdentity $Path $Root
  if ($identity.bytes -ne $Bytes -or $identity.sha256 -ne $Sha256) {
    throw "Installed smoke file identity changed."
  }
  return $identity
}

Export-ModuleMember -Function @(
  "Assert-InstalledSmokePath",
  "Get-InstalledSmokeFileIdentity",
  "Assert-InstalledSmokeFileIdentity"
)
