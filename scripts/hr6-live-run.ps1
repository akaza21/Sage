[CmdletBinding()]
param(
  [Parameter(Mandatory)][string]$SourceRoot,
  [Parameter(Mandatory)][string]$SageExecutable,
  [int]$Port = 9331,
  [int]$DeadlineMs = 600000,
  [string]$SageLogDirectory,
  [string]$SageRustLog = "warn",
  [switch]$IncludeOpenCode
)

$ErrorActionPreference = "Stop"
$SourceRoot = [IO.Path]::GetFullPath($SourceRoot)
. (Join-Path $PSScriptRoot "hr6-disposable.ps1")
. (Join-Path $PSScriptRoot "hr6-null-sink.ps1")
. (Join-Path $PSScriptRoot "hr6-process-receipt.ps1")
. (Join-Path $PSScriptRoot "hr6-sage-log.ps1")
if ($Port -lt 1 -or $Port -gt 65535) { throw "Port must be between 1 and 65535." }
if ($Port -eq 1420) { throw "The CDP port cannot be Vite's port 1420." }
$captureRequested = $PSBoundParameters.ContainsKey("SageLogDirectory") -and -not [string]::IsNullOrWhiteSpace($SageLogDirectory)
if ($PSBoundParameters.ContainsKey("SageRustLog") -and -not $captureRequested) { throw "SageRustLog requires SageLogDirectory." }
Assert-SageRustLog $SageRustLog
$openCodeFlag = if ($IncludeOpenCode) { "true" } else { "false" }

function Assert-LoopbackPortAvailable([int]$Candidate) {
  $listener = [System.Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, $Candidate)
  try {
    $listener.Start()
  } catch {
    throw "Loopback port $Candidate is already in use."
  } finally {
    $listener.Stop()
  }
}

Assert-LoopbackPortAvailable 1420
Assert-LoopbackPortAvailable $Port
$stamp = [DateTime]::UtcNow.ToString("yyyyMMddTHHmmssZ")
$output = Join-Path $SourceRoot "work\hr6\$stamp"
$disposableProfile = Resolve-DisposableChildPath $output "profile"
$scratch = Resolve-DisposableChildPath $output "scratch"
New-Item -ItemType Directory -Path $output, $disposableProfile, $scratch | Out-Null
New-Item -ItemType Directory -Path (Join-Path $disposableProfile "durable"), (Join-Path $disposableProfile "webview") | Out-Null
[IO.File]::WriteAllBytes((Join-Path $disposableProfile ".sage-disposable-profile-v1"), [Text.Encoding]::ASCII.GetBytes("sage-disposable-profile-v1`n"))
Set-Content -LiteralPath (Join-Path $scratch "README.md") -Value "# HR-6 Scratch" -NoNewline -Encoding utf8
Set-Content -LiteralPath (Join-Path $scratch "alpha.txt") -Value "read-only harness probe" -NoNewline -Encoding utf8
$sink = Get-NullSinkTargets
$sageOut = $sink.stdout
$sageErr = $sink.stderr
$sageRustLog = "warn"
if ($captureRequested) {
  $logDirectory = Resolve-SageLogDirectory $output $SageLogDirectory
  New-Item -ItemType Directory -Path $logDirectory -Force | Out-Null
  $sageOut = Join-Path $logDirectory "sage.stdout.log"
  $sageErr = Join-Path $logDirectory "sage.stderr.log"
  $sageRustLog = $SageRustLog
  Assert-SageLogTargets $output $logDirectory
}
$vite = $null
$sage = $null
$priorWebview2AdditionalBrowserArguments = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
$priorSageDisposableProfileRoot = $env:SAGE_DISPOSABLE_PROFILE_ROOT
$priorSageProjectRoot = $env:SAGE_PROJECT_ROOT
$priorRustLog = $env:RUST_LOG
try {
  $viteEntry = Join-Path $SourceRoot "node_modules\vite\bin\vite.js"
  if (-not (Test-Path -LiteralPath $viteEntry)) { throw "Vite entrypoint is missing." }
  $vite = Start-Process -FilePath "node.exe" -ArgumentList @($viteEntry, "--host", "127.0.0.1", "--port", "1420", "--strictPort") -WorkingDirectory $SourceRoot -WindowStyle Hidden -RedirectStandardOutput $sink.stdout -RedirectStandardError $sink.stderr -PassThru
  $null = $vite.Handle
  $readyAt = [DateTime]::UtcNow.AddSeconds(45)
  do {
    try {
      $response = Invoke-WebRequest -UseBasicParsing -Uri "http://127.0.0.1:1420/" -TimeoutSec 2
      if ($response.StatusCode -eq 200) { break }
    } catch {}
    Start-Sleep -Milliseconds 250
  } while ([DateTime]::UtcNow -lt $readyAt)
  if ($null -eq $response -or $response.StatusCode -ne 200) { throw "Vite did not become ready." }
  $env:SAGE_DISPOSABLE_PROFILE_ROOT = $disposableProfile
  $env:SAGE_PROJECT_ROOT = $scratch
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-address=127.0.0.1 --remote-debugging-port=$Port"
  $env:RUST_LOG = $sageRustLog
  if ($captureRequested) { Assert-SageLogTargets $output $logDirectory }
  $sage = Start-Process -FilePath $SageExecutable -WorkingDirectory (Split-Path -Parent $SageExecutable) -RedirectStandardOutput $sageOut -RedirectStandardError $sageErr -PassThru
  $null = $sage.Handle
  & node (Join-Path $SourceRoot "scripts\hr6-live-run.mjs") --port "$Port" --output $output --deadline-ms "$DeadlineMs" --include-opencode $openCodeFlag
  if ($LASTEXITCODE -ne 0) { throw "HR-6 CDP driver failed." }
} finally {
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $priorWebview2AdditionalBrowserArguments
  $env:SAGE_DISPOSABLE_PROFILE_ROOT = $priorSageDisposableProfileRoot
  $env:SAGE_PROJECT_ROOT = $priorSageProjectRoot
  $env:RUST_LOG = $priorRustLog
  $sageExit = Get-ProcessExitReceipt $sage
  $viteExit = Get-ProcessExitReceipt $vite
  if ($null -ne $sage -and -not $sage.HasExited) { & taskkill.exe /pid $sage.Id /t /f | Out-Null }
  if ($null -ne $vite -and -not $vite.HasExited) { & taskkill.exe /pid $vite.Id /t /f | Out-Null }
  [ordered]@{ sage = $sageExit; vite = $viteExit; completedAt = [DateTime]::UtcNow.ToString("o") } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $output "launch-receipt.json") -Encoding utf8
  try {
    Remove-DisposableDirectory $scratch $output
  } finally {
    Remove-DisposableDirectory $disposableProfile $output
  }
}
Write-Output $output
