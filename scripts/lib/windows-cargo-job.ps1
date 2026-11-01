param(
  [Parameter(Mandatory)][string]$ModulePath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$MaximumRequestBytes = 262144
$MaximumProofBytes = 16384
$MaximumEnvironmentEntries = 512
$MaximumEnvironmentBytes = 131072
$MaximumArguments = 128
$MaximumArgumentBytes = 131072
$MaximumActiveProcesses = 256

function Test-BoundedInteger {
  param($Value, [long]$Minimum, [long]$Maximum)
  return (($Value -is [int]) -or ($Value -is [long])) -and
    ([long]$Value -ge $Minimum) -and ([long]$Value -le $Maximum)
}

if (-not [IO.Path]::IsPathRooted($ModulePath)) { throw "module-path-invalid" }
$modulePhysical = (Get-Item -LiteralPath $ModulePath).FullName
$repositoryRoot = (Get-Item -LiteralPath (Join-Path (Split-Path -Parent $PSScriptRoot) "..")).FullName
if (-not $modulePhysical.StartsWith($repositoryRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
  throw "module-path-escaped"
}
$nativePhysical = (Get-Item -LiteralPath (Join-Path (Split-Path -Parent $modulePhysical) "WindowsCargoJob.Native.cs")).FullName
if (-not $nativePhysical.StartsWith($repositoryRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
  throw "native-path-escaped"
}
Import-Module $modulePhysical -Force

$line = [Console]::In.ReadLine()
if ($null -eq $line -or [Text.Encoding]::UTF8.GetByteCount($line) -gt $MaximumRequestBytes) {
  throw "request-size-invalid"
}
$request = $line | ConvertFrom-Json
if ($request.schemaVersion -ne 1 -or -not ([string]$request.requestNonce -match '^[0-9a-f-]{36}$') -or
    -not [IO.Path]::IsPathRooted([string]$request.executablePath) -or
    -not [IO.Path]::IsPathRooted([string]$request.cwd) -or
    $request.arguments.Count -gt $MaximumArguments -or
    $request.environment.Count -gt $MaximumEnvironmentEntries) {
  throw "request-shape-invalid"
}
if (-not (Test-BoundedInteger $request.deadlineEpochMs 0 ([long]::MaxValue)) -or
    -not (Test-BoundedInteger $request.terminationGraceMs 1 5000) -or
    -not (Test-BoundedInteger $request.settlementTimeoutMs 1 60000) -or
    -not (Test-BoundedInteger $request.maximumLineBytes 1 1048576) -or
    -not (Test-BoundedInteger $request.maximumOutputBytes 1 16777216)) {
  throw "request-bounds-invalid"
}
$pipePrefix = "\\.\pipe\"
if (-not ([string]$request.controlPipe).StartsWith($pipePrefix, [StringComparison]::Ordinal) -or
    -not ([string]$request.controlPipe).Substring($pipePrefix.Length).StartsWith("sage-cargo-", [StringComparison]::Ordinal) -or
    -not ([string]$request.controlNonce -match '^[0-9a-f]{64}$')) { throw "control-invalid" }
$pipeName = ([string]$request.controlPipe).Substring($pipePrefix.Length)
$argumentBytes = 0
foreach ($value in $request.arguments) { $argumentBytes += [Text.Encoding]::UTF8.GetByteCount([string]$value) }
$environmentBytes = 0
foreach ($value in $request.environment) { $environmentBytes += [Text.Encoding]::UTF8.GetByteCount([string]$value) }
if ($argumentBytes -gt $MaximumArgumentBytes -or $environmentBytes -gt $MaximumEnvironmentBytes) {
  throw "request-content-oversize"
}

$deadlineUtc = [DateTimeOffset]::FromUnixTimeMilliseconds([long]$request.deadlineEpochMs).UtcDateTime
$result = Invoke-CargoValidationJob `
  -RequestNonce ([string]$request.requestNonce) `
  -ControlPipeName $pipeName `
  -ControlNonce ([string]$request.controlNonce) `
  -ExecutablePath ([string]$request.executablePath) `
  -Arguments ([string[]]$request.arguments) `
  -WorkingDirectory ([string]$request.cwd) `
  -EnvironmentEntries ([string[]]$request.environment) `
  -DeadlineUtc $deadlineUtc `
  -TerminationGraceMilliseconds ([int]$request.terminationGraceMs) `
  -SettlementMilliseconds ([int]$request.settlementTimeoutMs) `
  -MaximumLineBytes ([int]$request.maximumLineBytes) `
  -MaximumOutputBytes ([int]$request.maximumOutputBytes) `
  -MaximumActiveProcesses $MaximumActiveProcesses

$json = $result | ConvertTo-Json -Compress -Depth 4
# The consumer decodes every protocol line as UTF-8, so the proof bypasses the
# console's code page just as the native protocol frames do.
$proofBytes = [Text.UTF8Encoding]::new($false).GetBytes($json + "`n")
if ($proofBytes.Length -gt $MaximumProofBytes) { throw "proof-oversize" }
$standardOutput = [Console]::OpenStandardOutput()
$standardOutput.Write($proofBytes, 0, $proofBytes.Length)
$standardOutput.Flush()
[Environment]::Exit(0)
