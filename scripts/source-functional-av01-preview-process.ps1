[CmdletBinding()]
param(
  [ValidateSet("capture", "absent")][string]$Mode,
  [string]$FixtureRoot,
  [string]$StartupPath,
  [string]$Nonce,
  [string]$SageProcessId,
  [string]$SageCreationDate,
  [string]$SageExecutablePath,
  [string]$IdentityJson
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Fail([string]$Code) { throw "av01-preview-process-$Code" }

function Absolute([string]$Value) {
  if ([string]::IsNullOrWhiteSpace($Value) -or -not [IO.Path]::IsPathRooted($Value)) { return $false }
  try { return [IO.Path]::GetFullPath($Value) -eq $Value } catch { return $false }
}

function ExactKeys($Value, [string[]]$Keys) {
  if ($null -eq $Value -or $Value -is [string] -or $Value -is [array]) { return $false }
  $actual = @($Value.PSObject.Properties.Name | Sort-Object) -join "`0"
  $expected = @($Keys | Sort-Object) -join "`0"
  return $actual -eq $expected
}

function Strict-ProcessId($Value, [string]$Code) {
  if ($Value -isnot [int] -and $Value -isnot [long]) { Fail $Code }
  $text = [string]$Value
  if ($text -notmatch "^[1-9]\d{0,9}$") { Fail $Code }
  try { $parsed = [int]$text } catch { Fail $Code }
  if ($parsed -le 0) { Fail $Code }
  return $parsed
}

function Strict-ArgumentProcessId([string]$Value, [string]$Code) {
  if ($Value -notmatch "^[1-9]\d{0,9}$") { Fail $Code }
  try { $parsed = [int]$Value } catch { Fail $Code }
  if ($parsed -le 0) { Fail $Code }
  return $parsed
}

function Assert-PlainFixtureAncestors {
  $current = $FixtureRoot
  while ($true) {
    if (-not (Test-Path -LiteralPath $current -PathType Container)) { Fail "startup-path" }
    $item = Get-Item -LiteralPath $current -Force
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { Fail "startup-path" }
    $parent = [IO.Path]::GetDirectoryName($current)
    if ([string]::IsNullOrEmpty($parent) -or $parent -eq $current) { break }
    $current = $parent
  }
}

function Read-Startup {
  if (-not (Absolute $FixtureRoot) -or -not (Absolute $StartupPath) -or
      [IO.Path]::GetDirectoryName($StartupPath) -ne $FixtureRoot -or
      [IO.Path]::GetFileName($StartupPath) -ne "av01-preview-startup.json" -or
      -not (Test-Path -LiteralPath $StartupPath -PathType Leaf)) { Fail "startup-path" }
  Assert-PlainFixtureAncestors
  $item = Get-Item -LiteralPath $StartupPath -Force
  if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or $item.Length -gt 4096) { Fail "startup-path" }
  try { $value = Get-Content -LiteralPath $StartupPath -Raw | ConvertFrom-Json } catch { Fail "startup-json" }
  $processId = Strict-ProcessId $value.processId "startup-json"
  if (-not (ExactKeys $value @("schemaVersion", "processId", "nonce", "case", "startedAt")) -or
      $value.schemaVersion -ne 1 -or $processId -le 0 -or
      $value.nonce -ne $Nonce -or $Nonce -notmatch "^[0-9a-f]{64}$" -or
      $value.case -notin @("starting-active", "health-failed", "exited") -or
      ([string]$value.startedAt).Length -lt 1 -or ([string]$value.startedAt).Length -gt 128) { Fail "startup-json" }
  return $value
}

function Read-Identity {
  if ([string]::IsNullOrWhiteSpace($IdentityJson) -or $IdentityJson.Length -gt 8192) { Fail "identity-json" }
  try { $identity = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($IdentityJson)) | ConvertFrom-Json } catch { Fail "identity-json" }
  $processId = Strict-ProcessId $identity.processId "identity-json"
  $sageProcessId = Strict-ProcessId $identity.sageProcessId "identity-json"
  if (-not (ExactKeys $identity @("schemaVersion", "nonce", "processId", "executablePath", "creationDate", "sageProcessId")) -or
      $identity.schemaVersion -ne 1 -or $identity.nonce -ne $Nonce -or $processId -le 0 -or
      -not (Absolute ([string]$identity.executablePath)) -or [string]$identity.creationDate.Length -lt 1 -or
      ([string]$identity.creationDate).Length -gt 128 -or $identity.creationDate -notmatch "^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$" -or
      $sageProcessId -ne $ParsedSageProcessId) { Fail "identity-json" }
  return $identity
}

Import-Module (Join-Path $PSScriptRoot "footprint/ProcessIdentity.psm1") -Force

$ParsedSageProcessId = Strict-ArgumentProcessId $SageProcessId "sage-identity"
if (-not (Absolute $SageExecutablePath) -or [string]::IsNullOrWhiteSpace($SageCreationDate) -or ([string]$SageCreationDate).Length -gt 128 -or $SageCreationDate -notmatch "^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$") {
  Fail "sage-identity"
}
$startup = Read-Startup
$rows = @(Get-FootprintProcessSnapshot)
$sage = @($rows | Where-Object {
  [int]$_.ProcessId -eq $ParsedSageProcessId
})
if ($sage.Count -ne 1) { Fail "sage-identity" }
try { [void](Get-FootprintRootIdentity $ParsedSageProcessId $SageExecutablePath $SageCreationDate $rows) } catch { Fail "sage-identity" }

if ($Mode -eq "capture") {
  $descendants = @(Get-FootprintDescendants $ParsedSageProcessId $rows)
  $candidates = @($descendants | Where-Object { [int]$_.ProcessId -eq [int]$startup.processId })
  if ($candidates.Count -ne 1 -or [int]$candidates[0].ProcessId -eq $ParsedSageProcessId) { Fail "fixture-not-owned" }
  $row = $candidates[0]
  try { $fixtureIdentity = ConvertTo-FootprintProcessIdentity $row } catch { Fail "fixture-not-owned" }
  [ordered]@{
    schemaVersion = 1
    nonce = $Nonce
    processId = [int]$fixtureIdentity.processId
    executablePath = [string]$fixtureIdentity.executablePath
    creationDate = [string]$fixtureIdentity.creationDate
    sageProcessId = [int]$ParsedSageProcessId
  } | ConvertTo-Json -Compress
  exit 0
}

$identity = Read-Identity
if ([int]$identity.processId -ne [int]$startup.processId) { Fail "identity-json" }
$same = @($rows | Where-Object {
  if ([int]$_.ProcessId -ne [int]$identity.processId) { return $false }
  try {
    $current = ConvertTo-FootprintProcessIdentity $_
    return $current.executablePath -eq [string]$identity.executablePath -and $current.creationDate -eq [string]$identity.creationDate
  } catch { return $false }
})
if ($same.Count -ne 0) { Fail "fixture-still-live" }
[ordered]@{
  schemaVersion = 1
  absent = $true
  nonce = $Nonce
  processId = [int]$identity.processId
  executablePath = [string]$identity.executablePath
  creationDate = [string]$identity.creationDate
  sageProcessId = [int]$ParsedSageProcessId
} | ConvertTo-Json -Compress
