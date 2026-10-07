$SageLogDepthLimit = 8
$SageLogTargetNames = @("sage.stdout.log", "sage.stderr.log")

function Test-SageLogReparsePoint([string]$Path) {
  $item = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
  if ($null -eq $item) { return $false }
  return (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)
}

function Assert-SageLogChain([string]$Output, [string]$Path) {
  $separators = @([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
  $resolvedOutput = [IO.Path]::GetFullPath($Output).TrimEnd($separators)
  if (Test-SageLogReparsePoint $resolvedOutput) { throw "The Sage log directory cannot traverse a reparse point." }
  $prefix = "$resolvedOutput$([IO.Path]::DirectorySeparatorChar)"
  if (-not $Path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { throw "The Sage log directory must be a child of the run output." }
  $parts = $Path.Substring($prefix.Length).Split($separators, [StringSplitOptions]::RemoveEmptyEntries)
  if ($parts.Count -lt 1 -or $parts.Count -gt $SageLogDepthLimit) { throw "The Sage log directory must be at most $SageLogDepthLimit levels below the run output." }
  $cursor = $resolvedOutput
  foreach ($part in $parts) {
    $cursor = Join-Path $cursor $part
    if (Test-SageLogReparsePoint $cursor) { throw "The Sage log directory cannot traverse a reparse point." }
  }
}

function Resolve-SageLogDirectory([string]$Output, [string]$Candidate) {
  $resolvedOutput = [IO.Path]::GetFullPath($Output).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
  $prefix = "$resolvedOutput$([IO.Path]::DirectorySeparatorChar)"
  $full = if ([IO.Path]::IsPathRooted($Candidate)) { [IO.Path]::GetFullPath($Candidate) } else { [IO.Path]::GetFullPath((Join-Path $resolvedOutput $Candidate)) }
  if (-not $full.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { throw "The Sage log directory must be a child of the run output." }
  $resolved = (Resolve-DisposableChildPath $resolvedOutput $full.Substring($prefix.Length)).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
  if ([String]::Equals($resolved, $resolvedOutput, [StringComparison]::OrdinalIgnoreCase)) { throw "The Sage log directory must be a child of the run output." }
  foreach ($reserved in @("profile", "scratch")) {
    $reservedPath = Resolve-DisposableChildPath $resolvedOutput $reserved
    $reservedPrefix = "$reservedPath$([IO.Path]::DirectorySeparatorChar)"
    if ([String]::Equals($resolved, $reservedPath, [StringComparison]::OrdinalIgnoreCase) -or $resolved.StartsWith($reservedPrefix, [StringComparison]::OrdinalIgnoreCase)) {
      throw "The Sage log directory cannot be a disposable directory."
    }
  }
  Assert-SageLogChain $resolvedOutput $resolved
  return $resolved
}

function Assert-SageLogTargets([string]$Output, [string]$Directory) {
  $resolved = Resolve-SageLogDirectory $Output $Directory
  foreach ($name in $SageLogTargetNames) {
    $target = Join-Path $resolved $name
    if ($null -ne (Get-Item -LiteralPath $target -Force -ErrorAction SilentlyContinue)) { throw "The Sage log targets must not already exist." }
  }
}

function Assert-SageRustLog([string]$Value) {
  if ($Value -notmatch '^[A-Za-z0-9_,=:.\-]{1,256}\z') { throw "SageRustLog must be a plain RUST_LOG filter." }
}
