param([Parameter(Mandatory)][string]$ModulePath)

$ErrorActionPreference = "Stop"

function Assert-Throws([scriptblock]$Action) {
  try {
    & $Action
  } catch {
    return
  }
  throw "Expected the action to throw."
}

. $ModulePath
$root = Join-Path ([IO.Path]::GetTempPath()) "sage-hr6-disposable-$([Guid]::NewGuid())"
$output = Join-Path $root "run"
$external = Join-Path $root "external"
$profile = Resolve-DisposableChildPath $output "profile"
$scratch = Resolve-DisposableChildPath $output "scratch"
New-Item -ItemType Directory -Path $profile, $scratch, $external | Out-Null
$sentinel = Join-Path $external "sentinel.txt"
[IO.File]::WriteAllText($sentinel, "external")

try {
  Assert-Throws { Resolve-DisposableChildPath $output ".." }
  Assert-Throws { Remove-DisposableDirectory $output $output }
  Assert-Throws { Remove-DisposableDirectory (Join-Path $output "unrecognized") $output }

  [IO.Directory]::Delete($profile, $true)
  New-Item -ItemType Junction -Path $profile -Target $external | Out-Null
  Assert-Throws { Remove-DisposableDirectory $profile $output }
  if (-not (Test-Path -LiteralPath $sentinel)) { throw "Top-level junction target was modified." }

  [IO.Directory]::Delete($profile, $false)
  New-Item -ItemType Directory -Path $profile | Out-Null
  New-Item -ItemType Junction -Path (Join-Path $profile "nested") -Target $external | Out-Null
  Remove-DisposableDirectory $profile $output
  if (Test-Path -LiteralPath $profile) { throw "Profile was not removed." }
  if (-not (Test-Path -LiteralPath $sentinel)) { throw "Nested junction target was modified." }

  Remove-DisposableDirectory $scratch $output
  if (Test-Path -LiteralPath $scratch) { throw "Scratch was not removed." }
} finally {
  if (Test-Path -LiteralPath $root) { [IO.Directory]::Delete($root, $true) }
}
