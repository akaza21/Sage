param([Parameter(Mandatory)][string]$ModulePath)

$ErrorActionPreference = "Stop"

function Assert-True($Condition, [string]$Message) {
  if (-not $Condition) { throw $Message }
}

. $ModulePath

$sink = Get-NullSinkTargets
Assert-True ($sink.stdout -ne $sink.stderr) "Start-Process rejects identical redirect targets."

$root = Join-Path ([IO.Path]::GetTempPath()) "sage-hr6-null-sink-$([Guid]::NewGuid())"
New-Item -ItemType Directory -Path $root | Out-Null
$previous = (Get-Location).Path
Set-Location $root
try {
  $chatty = Start-Process -FilePath "cmd.exe" -ArgumentList "/c", "for /L %i in (1,1,5000) do @echo hr6-null-sink-volume-%i" -RedirectStandardOutput $sink.stdout -RedirectStandardError $sink.stderr -PassThru -WindowStyle Hidden
  Assert-True ($null -ne $chatty) "Both null-sink targets were not accepted together."
  $null = $chatty.Handle
  Assert-True ($chatty.WaitForExit(60000)) "A high-volume writer on the null sink did not exit."
  Assert-True ($chatty.ExitCode -eq 0) "A high-volume writer on the null sink failed."

  $failing = Start-Process -FilePath "cmd.exe" -ArgumentList "/c", "echo hr6-null-sink-error 1>&2 & exit 7" -RedirectStandardOutput $sink.stdout -RedirectStandardError $sink.stderr -PassThru -WindowStyle Hidden
  $null = $failing.Handle
  Assert-True ($failing.WaitForExit(60000)) "A failing writer on the null sink did not exit."
  Assert-True ($failing.ExitCode -eq 7) "Discarding streams lost the exit classification."

  $created = Get-ChildItem -LiteralPath $root -Recurse -Force
  Assert-True ($null -eq $created) "Discarded streams created a file on disk."
  Assert-True (-not [IO.File]::Exists((Join-Path $root "NUL"))) "A literal NUL file was created."
} finally {
  Set-Location $previous
  if (Test-Path -LiteralPath $root) { [IO.Directory]::Delete($root, $true) }
}
