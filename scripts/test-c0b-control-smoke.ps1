param(
  [Parameter(Mandatory)][string]$ControlExecutable,
  [string]$NodePath = "node.exe"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

Import-Module (Join-Path $PSScriptRoot "footprint/C0BObservation.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "footprint/ScenarioEvidence.psm1") -Force

$node = (Get-Command $NodePath -CommandType Application -ErrorAction Stop).Source
$boundedNodeRunner = (Resolve-Path (Join-Path $PSScriptRoot "footprint/bounded-node-runner.mjs")).Path
$runValidator = ([Uri](Resolve-Path (Join-Path $PSScriptRoot "ade0006a-c0b-footprint-evidence.mjs")).Path).AbsoluteUri
$executable = Get-Item -LiteralPath $ControlExecutable -Force
if ($executable.PSIsContainer -or
    -not [string]::Equals($executable.Extension, ".exe", [StringComparison]::OrdinalIgnoreCase)) {
  throw "The C0-B control smoke requires one executable file."
}

$binder = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "ade0006a-c0b-campaign-launch.mjs"))
if (-not (Test-Path -LiteralPath $binder -PathType Leaf)) { throw "Campaign observation binder is missing." }
$previousErrorAction = $ErrorActionPreference
$ErrorActionPreference = "Continue"
try {
  $planOutput = & $node $binder plan shell 2>&1
  $planExit = $LASTEXITCODE
} finally {
  $ErrorActionPreference = $previousErrorAction
}
if ($planExit -ne 0) {
  $message = (@($planOutput) | ForEach-Object { "$_" }) -join "`n"
  throw "Admitted shell state plan is missing: $message"
}
$statePlan = (($planOutput | ForEach-Object { "$_" }) -join "`n") | ConvertFrom-Json

$root = Join-Path ([IO.Path]::GetTempPath()) ("sage-c0b-control-smoke-" + [Guid]::NewGuid().ToString("N"))
$profiles = Join-Path $root "profiles"
[void][IO.Directory]::CreateDirectory($profiles)
$requestPath = Join-Path $root "request.json"
$outputPath = Join-Path $root "output.json"
$wrongBytesRequestPath = Join-Path $root "wrong-bytes-request.json"
$wrongBytesOutputPath = Join-Path $root "wrong-bytes-output.json"
$executableSha256 = (Get-FileHash -LiteralPath $executable.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
$bindingHasher = [Security.Cryptography.SHA256]::Create()
try {
  $bindingSha256 = ([BitConverter]::ToString(
    $bindingHasher.ComputeHash([Text.Encoding]::UTF8.GetBytes("sage-c0b-control-smoke-binding-v1"))
  )).Replace("-", "").ToLowerInvariant()
} finally {
  $bindingHasher.Dispose()
}
if ($bindingSha256 -eq $executableSha256) {
  throw "The C0-B control smoke binding must differ from the raw executable hash."
}

# Campaign observation executables must come from the Node composer, not a raw request.
# The state plan must come from the admitted catalog (`plan shell`), not a reconstructed twin.
$request = [ordered]@{
  schemaVersion = 1
  stateId = "shell"
  participant = "control"
  launchOrdinal = 1
  executable = [ordered]@{
    path = $executable.FullName
    sha256 = $executableSha256
    bytes = [long]$executable.Length
    bindingSha256 = $bindingSha256
  }
  profile = [ordered]@{ campaignRoot = $profiles; id = "control-shell-0001" }
  projectRoot = $null
  expectedWindowTitle = "Sage Footprint Control"
  cadence = [ordered]@{
    settleAfterSemanticReadyMs = 100
    snapshotIntervalMs = 250
    snapshotCount = 2
  }
  statePlan = $statePlan
  outputPath = $outputPath
}

try {
  $wrongBytesRequest = ($request | ConvertTo-Json -Compress -Depth 10) | ConvertFrom-Json
  $wrongBytesRequest.executable.bytes = [long]$executable.Length + 1
  $wrongBytesRequest.outputPath = $wrongBytesOutputPath
  [IO.File]::WriteAllText(
    $wrongBytesRequestPath,
    ($wrongBytesRequest | ConvertTo-Json -Compress -Depth 10),
    [Text.UTF8Encoding]::new($false)
  )
  $wrongBytesRejected = $false
  try {
    [void](Invoke-C0BObservationRequest $wrongBytesRequestPath)
  } catch {
    $wrongBytesRejected = $_.Exception.Message -eq "Observation executable identity is invalid."
    if (-not $wrongBytesRejected) { throw }
  }
  if (-not $wrongBytesRejected -or (Test-Path -LiteralPath $wrongBytesOutputPath)) {
    throw "The C0-B collector did not reject mismatched executable bytes before launch."
  }
  [IO.File]::WriteAllText(
    $requestPath,
    ($request | ConvertTo-Json -Compress -Depth 10),
    [Text.UTF8Encoding]::new($false)
  )
  $status = Invoke-C0BObservationRequest $requestPath
  $result = Get-Content -LiteralPath $outputPath -Raw | ConvertFrom-Json
  if ($status.status -ne "passed" -or
      $result.stateVerified -ne $true -or
      $result.normalSageDataMutations -ne 0 -or
      $result.orphanProcessesAfterClose -ne 0 -or
      @($result.snapshots).Count -ne 2) {
    throw "The C0-B control smoke result is incomplete."
  }
  $validationResultPath = Join-Path $root "validation-result.json"
  $validatorExpression = @"
import fs from "node:fs";
import { validatePackagedRun } from "$runValidator";
const run = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
const errors = validatePackagedRun({
  run,
  launchOrdinal: 1,
  executableBindingSha256: process.argv[2],
  cadence: {
    settleAfterSemanticReadyMs: 100,
    snapshotIntervalMs: 250,
    snapshotCount: 2,
    minimumSamplingWindowMs: 250,
  },
  frequencyHz: Number(process.argv[3]),
  label: "controlSmoke",
  identities: { processes: new Set(), profiles: new Set() },
});
if (errors.length > 0) {
  console.error(JSON.stringify(errors));
  process.exit(1);
}
console.log("c0b-run-valid");
"@
  $validation = Invoke-FootprintBoundedProcess $node $boundedNodeRunner $node @(
    "--input-type=module",
    "--eval",
    $validatorExpression,
    $outputPath,
    $bindingSha256,
    [string][Diagnostics.Stopwatch]::Frequency
  ) $validationResultPath 120000 65536
  if (-not $validation.stdout.Contains("c0b-run-valid")) {
    throw "The real C0-B control output failed the public run validator."
  }
  $maxSnapshotOffsetMs = @($result.snapshots | ForEach-Object {
    (([long]$_.capturedTick - [long]$_.dueTick) * 1000.0) / [Diagnostics.Stopwatch]::Frequency
  } | Measure-Object -Maximum).Maximum
  [ordered]@{
    status = "passed"
    stateId = $status.stateId
    snapshots = @($result.snapshots).Count
    semanticActions = 5
    maxSnapshotOffsetMs = [Math]::Round($maxSnapshotOffsetMs, 3)
  } | ConvertTo-Json -Compress
} finally {
  if (Test-Path -LiteralPath $root) {
    Remove-Item -LiteralPath $root -Recurse -Force
  }
}
