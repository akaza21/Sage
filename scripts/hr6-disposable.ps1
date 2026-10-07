function Resolve-DisposableChildPath([string]$Parent, [string]$ChildName) {
  $resolvedParent = [IO.Path]::GetFullPath($Parent).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
  $resolvedChild = [IO.Path]::GetFullPath((Join-Path $resolvedParent $ChildName))
  $prefix = "$resolvedParent$([IO.Path]::DirectorySeparatorChar)"
  if (-not $resolvedChild.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Disposable path must be a child of its run output."
  }
  return $resolvedChild
}

function Remove-DisposableDirectory([string]$Path, [string]$Parent) {
  $resolvedParent = [IO.Path]::GetFullPath($Parent).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
  $resolvedPath = [IO.Path]::GetFullPath($Path)
  $profilePath = Resolve-DisposableChildPath $resolvedParent "profile"
  $scratchPath = Resolve-DisposableChildPath $resolvedParent "scratch"
  $prefix = "$resolvedParent$([IO.Path]::DirectorySeparatorChar)"
  if (-not $resolvedPath.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to remove a path outside the run output."
  }
  if (
    -not [String]::Equals($resolvedPath, $profilePath, [StringComparison]::OrdinalIgnoreCase) -and
    -not [String]::Equals($resolvedPath, $scratchPath, [StringComparison]::OrdinalIgnoreCase)
  ) {
    throw "Refusing to remove an unrecognized disposable path."
  }
  if (-not (Test-Path -LiteralPath $resolvedPath)) { return }
  if ((Get-Item -LiteralPath $resolvedPath -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
    throw "Refusing to remove a reparse-point disposable path."
  }
  $expiresAt = [DateTime]::UtcNow.AddSeconds(15)
  do {
    try {
      if (Test-Path -LiteralPath $resolvedPath) { [IO.Directory]::Delete($resolvedPath, $true) }
      return
    } catch {
      Start-Sleep -Milliseconds 500
    }
  } while ([DateTime]::UtcNow -lt $expiresAt)
  Write-Warning "Could not remove disposable directory: $resolvedPath"
}
