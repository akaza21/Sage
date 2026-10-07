param(
  [Parameter(Mandatory)][string]$ModulePath,
  [Parameter(Mandatory)][string]$DisposablePath
)

$ErrorActionPreference = "Stop"
. $DisposablePath
. $ModulePath

$failures = New-Object System.Collections.Generic.List[string]
$junctions = New-Object System.Collections.Generic.List[string]

function Add-Failure([string]$Message) { $failures.Add($Message) }

function Assert-Accepted([string]$Name, [scriptblock]$Action) {
  try { $null = & $Action } catch { Add-Failure "$Name should be accepted but threw: $($_.Exception.Message)" }
}

function Assert-Refused([string]$Name, [scriptblock]$Action) {
  $accepted = $false
  try { $null = & $Action; $accepted = $true } catch { }
  if ($accepted) { Add-Failure "$Name should be refused but was accepted." }
}

function New-TestJunction([string]$Path, [string]$Target) {
  New-Item -ItemType Junction -Path $Path -Target $Target -ErrorAction Stop | Out-Null
  $junctions.Add($Path)
}

$temp = (New-Item -ItemType Directory -Path (Join-Path $env:TEMP ("hr6-sage-log-" + [Guid]::NewGuid().ToString("N")))).FullName
try {
  $out = (New-Item -ItemType Directory -Path (Join-Path $temp "work\hr6\stamp") -Force).FullName
  $outside = (New-Item -ItemType Directory -Path (Join-Path $temp "outside")).FullName
  New-Item -ItemType Directory -Path (Join-Path $out "profile"), (Join-Path $out "scratch") | Out-Null

  try {
    $resolved = Resolve-SageLogDirectory $out "logs"
    if (-not [String]::Equals($resolved, (Join-Path $out "logs"), [StringComparison]::OrdinalIgnoreCase)) {
      Add-Failure "a relative log directory should resolve under the run output but produced $resolved."
    }
  } catch {
    Add-Failure "a relative log directory should be accepted but threw: $($_.Exception.Message)"
  }
  Assert-Accepted "a nested log directory" { Resolve-SageLogDirectory $out "logs\nested" }
  Assert-Refused "a traversal outside the run output" { Resolve-SageLogDirectory $out "..\elsewhere" }
  Assert-Refused "an absolute path outside the run output" { Resolve-SageLogDirectory $out $outside }
  Assert-Refused "the run output itself" { Resolve-SageLogDirectory $out "" }
  Assert-Refused "the disposable scratch directory" { Resolve-SageLogDirectory $out "scratch" }
  Assert-Refused "the disposable profile directory" { Resolve-SageLogDirectory $out "profile" }
  Assert-Refused "a log directory deeper than the component limit" { Resolve-SageLogDirectory $out "a\b\c\d\e\f\g\h\i" }

  New-TestJunction (Join-Path $out "link") $outside
  Assert-Refused "a log directory reached through an intermediate junction" { Resolve-SageLogDirectory $out "link\child" }
  Assert-Refused "a log directory that is itself a junction" { Resolve-SageLogDirectory $out "link" }

  $logs = (New-Item -ItemType Directory -Path (Join-Path $out "logs")).FullName
  Assert-Accepted "an empty log directory without existing targets" { Assert-SageLogTargets $out "logs" }
  Assert-Accepted "an absolute log directory inside the run output" { Assert-SageLogTargets $out $logs }

  $stdout = Join-Path $logs "sage.stdout.log"
  New-Item -ItemType File -Path $stdout | Out-Null
  Assert-Refused "an existing stdout log target" { Assert-SageLogTargets $out "logs" }
  Remove-Item -LiteralPath $stdout -Force

  $stderrLinked = $false
  try {
    New-TestJunction (Join-Path $logs "sage.stderr.log") $outside
    $stderrLinked = $true
  } catch {
    Write-Warning "skipping the linked stderr target case: $($_.Exception.Message)"
  }
  if ($stderrLinked) {
    Assert-Refused "a linked stderr log target" { Assert-SageLogTargets $out "logs" }
  }

  Assert-Accepted "a plain RUST_LOG filter" { Assert-SageRustLog "warn" }
  Assert-Accepted "a scoped RUST_LOG filter" { Assert-SageRustLog "sage=debug,warn" }
  Assert-Refused "a RUST_LOG filter with a trailing newline" { Assert-SageRustLog "warn`n" }
  Assert-Refused "a RUST_LOG filter with whitespace" { Assert-SageRustLog "a b" }
  Assert-Refused "a RUST_LOG filter with a command separator" { Assert-SageRustLog "warn; calc.exe" }
  Assert-Refused "an empty RUST_LOG filter" { Assert-SageRustLog "" }
} finally {
  foreach ($junction in $junctions) {
    if ([IO.Directory]::Exists($junction)) { [IO.Directory]::Delete($junction, $false) }
  }
  if ([IO.Directory]::Exists($temp)) { [IO.Directory]::Delete($temp, $true) }
}

if ($failures.Count -gt 0) {
  foreach ($failure in $failures) { Write-Output $failure }
  exit 1
}
Write-Output "hr6-sage-log checks passed"
