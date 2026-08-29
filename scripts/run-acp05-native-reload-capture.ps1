# ACP-05 collection-only wrapper: it launches and terminates no application.
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)] [string] $RepoRoot,
  [Parameter(Mandatory = $true)] [string] $SourceCommit,
  [Parameter(Mandatory = $true)] [string] $Executable,
  [Parameter(Mandatory = $true)] [string] $ExecutableSha256,
  [Parameter(Mandatory = $true)] [string] $CdpEndpoint,
  [Parameter(Mandatory = $true)] [string] $EvidenceRoot,
  [Parameter(Mandatory = $true)] [string] $MarkerLog,
  [string] $MarkerNonce,
  [string] $ProcessId,
  [string] $Receipt,
  [int] $DeadlineMs = 60000
)

$ErrorActionPreference = "Stop"
function Assert-Absolute([string] $Value, [string] $Name) {
  if ([string]::IsNullOrWhiteSpace($Value) -or -not [IO.Path]::IsPathRooted($Value)) { throw "acp05: $Name must be absolute" }
}
foreach ($entry in @(@($RepoRoot, "repo-root"), @($Executable, "executable"), @($EvidenceRoot, "evidence-root"), @($MarkerLog, "marker-log"))) { Assert-Absolute $entry[0] $entry[1] }
if ($PSBoundParameters.ContainsKey("Receipt")) { Assert-Absolute $Receipt "receipt" }
$scriptPath = Join-Path $PSScriptRoot "acp05-native-reload-capture.mjs"
if (-not (Test-Path -LiteralPath $scriptPath -PathType Leaf)) { throw "acp05: collector missing" }
$nodeArgs = @($scriptPath, "--repo-root", $RepoRoot, "--source-commit", $SourceCommit, "--executable", $Executable, "--executable-sha256", $ExecutableSha256, "--cdp-endpoint", $CdpEndpoint, "--evidence-root", $EvidenceRoot, "--marker-log", $MarkerLog, "--deadline-ms", "$DeadlineMs")
if ($PSBoundParameters.ContainsKey("MarkerNonce")) { $nodeArgs += @("--marker-nonce", $MarkerNonce) }
if ($PSBoundParameters.ContainsKey("ProcessId")) { $nodeArgs += @("--process-id", $ProcessId) }
if ($PSBoundParameters.ContainsKey("Receipt")) { $nodeArgs += @("--receipt", $Receipt) }
& node @nodeArgs
exit $LASTEXITCODE
