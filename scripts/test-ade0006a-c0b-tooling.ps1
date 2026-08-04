[CmdletBinding()]
param(
  [string]$NodePath = "node.exe"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$scriptRoot = $PSScriptRoot
$node = (Get-Command $NodePath -CommandType Application -ErrorAction Stop).Source
$runner = Join-Path $scriptRoot "footprint/bounded-node-runner.mjs"
$nodeTests = Join-Path $scriptRoot "validate-ade0006a-c0b.node-test.mjs"
$validators = @(
  (Join-Path $scriptRoot "validate-ade0006a-c0b-plan.mjs"),
  (Join-Path $scriptRoot "validate-ade0006a-c0b-results.mjs")
)
$staticSources = @(
  $validators +
  (Join-Path $scriptRoot "ade0006a-c0b-common.mjs") +
  (Join-Path $scriptRoot "ade0006a-c0b-contracts.mjs") +
  (Join-Path $scriptRoot "ade0006a-c0b-trust.mjs") +
  (Join-Path $scriptRoot "ade0006a-c0b-metrics.mjs") +
  (Join-Path $scriptRoot "ade0006a-c0b-runtime-evidence.mjs") +
  (Join-Path $scriptRoot "ade0006a-c0b-artifact-evidence.mjs")
)

foreach ($path in @($staticSources + $runner + $nodeTests)) {
  if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
    throw "Missing C0-B tooling file: $path"
  }
}

foreach ($sourcePath in $staticSources) {
  $source = Get-Content -LiteralPath $sourcePath -Raw
  foreach ($forbidden in @(
      "node:child_process", "child_process", "Start-Process", "cmd.exe",
      "powershell.exe", "execFile(", "execSync(", "spawn(", "spawnSync("
    )) {
    if ($source.Contains($forbidden)) {
      throw "C0-B validator contains forbidden process capability '$forbidden'."
    }
  }
}
foreach ($validator in $validators) {
  if (-not (Get-Content -LiteralPath $validator -Raw).Contains("--dry-run")) {
    throw "C0-B public validator does not require --dry-run."
  }
}

function ConvertTo-Base64Url {
  param([Parameter(Mandatory)][string]$Value)
  return [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Value)).TrimEnd("=").Replace("+", "-").Replace("/", "_")
}

function Invoke-BoundedNode {
  param(
    [Parameter(Mandatory)][string[]]$Arguments,
    [Parameter(Mandatory)][int]$ExpectedExitCode,
    [Parameter(Mandatory)][string]$ExpectedText
  )

  foreach ($argument in $Arguments) {
    if ($argument.Length -gt 4096 -or $argument.Contains('"')) {
      throw "C0-B Node argument is not safely bounded."
    }
  }
  $resultPath = Join-Path ([IO.Path]::GetTempPath()) ("sage-c0b-{0}.json" -f [Guid]::NewGuid().ToString("N"))
  $request = [ordered]@{
    executablePath = $node
    arguments = $Arguments
    timeoutMs = 120000
    maxOutputBytes = 1048576
    resultPath = $resultPath
  } | ConvertTo-Json -Compress
  $encoded = ConvertTo-Base64Url $request
  $process = [Diagnostics.Process]::new()
  $process.StartInfo = [Diagnostics.ProcessStartInfo]::new()
  $process.StartInfo.FileName = $node
  $process.StartInfo.Arguments = ('"{0}" {1}' -f $runner, $encoded)
  $process.StartInfo.UseShellExecute = $false
  $process.StartInfo.CreateNoWindow = $true
  $process.StartInfo.RedirectStandardOutput = $true
  $process.StartInfo.RedirectStandardError = $true
  $started = $false
  try {
    if (-not $process.Start()) {
      throw "C0-B bounded Node runner did not start."
    }
    $started = $true
    if (-not $process.WaitForExit(130000)) {
      $process.Kill()
      throw "C0-B bounded Node runner exceeded its deadline."
    }
    $runnerStdout = $process.StandardOutput.ReadToEnd()
    $runnerStderr = $process.StandardError.ReadToEnd()
    if ($process.ExitCode -ne 0 -or $runnerStdout.Length -gt 65536 -or $runnerStderr.Length -gt 65536) {
      throw "C0-B bounded Node runner failed or exceeded its own output bound."
    }
    if (-not (Test-Path -LiteralPath $resultPath -PathType Leaf)) {
      throw "C0-B bounded Node runner omitted its result."
    }
    $item = Get-Item -LiteralPath $resultPath
    if ($item.Length -lt 1 -or $item.Length -gt 2097152) {
      throw "C0-B bounded Node result exceeded its file bound."
    }
    $result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
    if ($result.exitCode -ne $ExpectedExitCode -or $result.terminalReason -or $result.spawnError) {
      throw "C0-B child exit did not match the expected disposition."
    }
    if (-not (([string]$result.stdout + [string]$result.stderr).Contains($ExpectedText))) {
      throw "C0-B child output omitted its expected bounded marker."
    }
    return $result
  } finally {
    if ($started -and -not $process.HasExited) {
      $process.Kill()
    }
    $process.Dispose()
    if (Test-Path -LiteralPath $resultPath) {
      Remove-Item -LiteralPath $resultPath -Force
    }
  }
}

[void](Invoke-BoundedNode @("--test", $nodeTests) 0 "# pass 10")
foreach ($validator in $validators) {
  [void](Invoke-BoundedNode @($validator) 1 "usage:")
}

[ordered]@{
  status = "passed"
  mode = "static-dry-run-only"
  validators = $validators.Count
  nodeTests = 10
  nativeExitSource = "bounded ProcessStartInfo result"
} | ConvertTo-Json -Compress
