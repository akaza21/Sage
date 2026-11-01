Set-StrictMode -Version Latest

if (-not ("SageValidation.CargoJob" -as [type])) {
  Add-Type -Path (Join-Path $PSScriptRoot "WindowsCargoJob.Native.cs")
}

function Wait-CargoJobZero {
  param(
    [Parameter(Mandatory)][SageValidation.CargoJob]$Job,
    [Parameter(Mandatory)][DateTime]$DeadlineUtc,
    [ValidateRange(0, 5000)][int]$ObservationMilliseconds = 100
  )
  $zeroSince = $null
  while ([DateTime]::UtcNow -lt $DeadlineUtc) {
    $active = [uint32]$Job.ActiveProcesses()
    if ($active -eq 0) {
      if ($null -eq $zeroSince) { $zeroSince = [DateTime]::UtcNow }
      elseif (([DateTime]::UtcNow - $zeroSince).TotalMilliseconds -ge $ObservationMilliseconds) {
        return $true
      }
    } elseif ($null -ne $zeroSince) {
      throw "tree-repopulated"
    }
    Start-Sleep -Milliseconds 10
  }
  return $false
}

function Request-CargoJobGracefulExit {
  param(
    [Parameter(Mandatory)][SageValidation.CargoJob]$Job,
    [Parameter(Mandatory)][DateTime]$DeadlineUtc
  )
  if ($Job.ActiveProcesses() -eq 0 -or -not $Job.RootRunning) { return }
  $startInfo = [Diagnostics.ProcessStartInfo]::new()
  $startInfo.FileName = Join-Path ([Environment]::SystemDirectory) "taskkill.exe"
  $startInfo.Arguments = "/PID $($Job.RootProcessId) /T"
  $startInfo.UseShellExecute = $false
  $startInfo.CreateNoWindow = $true
  $startInfo.RedirectStandardOutput = $true
  $startInfo.RedirectStandardError = $true
  $request = [Diagnostics.Process]::Start($startInfo)
  $stdoutDiscard = [SageValidation.BoundedDiscardStream]::new(65536)
  $stderrDiscard = [SageValidation.BoundedDiscardStream]::new(65536)
  $stdoutTask = $request.StandardOutput.BaseStream.CopyToAsync($stdoutDiscard)
  $stderrTask = $request.StandardError.BaseStream.CopyToAsync($stderrDiscard)
  try {
    while (-not $request.HasExited -and [DateTime]::UtcNow -lt $DeadlineUtc -and
        $Job.ActiveProcesses() -ne 0) {
      Start-Sleep -Milliseconds 10
    }
  } finally {
    if (-not $request.HasExited) {
      $request.Kill()
      $request.WaitForExit(500) | Out-Null
    }
    [Threading.Tasks.Task]::WaitAll(@($stdoutTask, $stderrTask), 500) | Out-Null
    $request.Dispose()
  }
}

function Invoke-CargoValidationJob {
  param(
    [string]$RequestNonce,
    [string]$ControlPipeName,
    [string]$ControlNonce,
    [string]$ExecutablePath,
    [string[]]$Arguments,
    [string]$WorkingDirectory,
    [string[]]$EnvironmentEntries,
    [DateTime]$DeadlineUtc,
    [int]$TerminationGraceMilliseconds,
    [int]$SettlementMilliseconds,
    [int]$MaximumLineBytes,
    [int]$MaximumOutputBytes,
    [uint32]$MaximumActiveProcesses
  )
  $stdout = $null
  $stderr = $null
  $job = $null
  $control = $null
  try {
    $control = [IO.Pipes.NamedPipeClientStream]::new(
      ".", $ControlPipeName, [IO.Pipes.PipeDirection]::In,
      [IO.Pipes.PipeOptions]::Asynchronous
    )
    $control.Connect(2000)
    $controlReader = [IO.StreamReader]::new($control)
    $abortTask = $controlReader.ReadLineAsync()
    $job = [SageValidation.CargoJob]::Start(
      $ExecutablePath, $Arguments, $WorkingDirectory, $EnvironmentEntries,
      $MaximumActiveProcesses, [ref]$stdout, [ref]$stderr
    )
    $protocol = [SageValidation.ProtocolWriter]::new(
      [Console]::OpenStandardOutput(), $RequestNonce
    )
    $stdoutCapture = [SageValidation.FramedCaptureStream]::new(
      $protocol, "stdout", $MaximumLineBytes, $MaximumOutputBytes
    )
    $stderrCapture = [SageValidation.FramedCaptureStream]::new(
      $protocol, "stderr", $MaximumLineBytes, $MaximumOutputBytes
    )
    $stdoutTask = $stdout.CopyToAsync($stdoutCapture)
    $stderrTask = $stderr.CopyToAsync($stderrCapture)
    $reason = $null
    $rootExitCode = $null
    $teardown = $null
    $teardownChecked = $false
    while ([DateTime]::UtcNow -lt $DeadlineUtc) {
      if ($abortTask.IsCompleted -and $abortTask.Result -eq "ABORT $ControlNonce") {
        $reason = "command-aborted"
        break
      }
      if ($null -ne $stdoutCapture.Overflow -or $null -ne $stderrCapture.Overflow) {
        $reason = "output-overflow"
        break
      }
      if (-not $teardownChecked -and -not $job.RootRunning) {
        # Capture the root outcome before the only compiled-in child teardown.
        # A successful teardown never turns a nonzero Cargo result into success.
        if ($null -eq $rootExitCode) { $rootExitCode = $job.ExitCode() }
        $teardownChecked = $true
        $teardown = $job.TryTeardownVerifiedVctip()
      }
      if ($job.ActiveProcesses() -eq 0) { break }
      Start-Sleep -Milliseconds 10
    }
    if ($null -eq $rootExitCode -and -not $job.RootRunning) {
      $rootExitCode = $job.ExitCode()
    }
    $stall = $null
    if ($null -eq $reason -and $job.ActiveProcesses() -ne 0) {
      $reason = "command-timeout"
      # Record what was still running at the deadline before teardown.
      $stall = [ordered]@{
        rootRunning = [bool]$job.RootRunning
        memberImages = @($job.MemberImageNames())
      }
    }
    if ($null -ne $reason) {
      $graceDeadline = [DateTime]::UtcNow.AddMilliseconds($TerminationGraceMilliseconds)
      Request-CargoJobGracefulExit $job $graceDeadline
      if (-not (Wait-CargoJobZero $job $graceDeadline 0)) { $job.Terminate() }
    }
    $settlementDeadline = [DateTime]::UtcNow.AddMilliseconds($SettlementMilliseconds)
    try {
      $settled = Wait-CargoJobZero $job $settlementDeadline 100
    } catch {
      if ($_.Exception.Message -ne "tree-repopulated") { throw }
      $reason = "tree-repopulated"
      $settled = $false
    }
    $activeProcesses = [uint32]$job.ActiveProcesses()
    $exitCode = if ($null -ne $rootExitCode) { $rootExitCode } else { $job.ExitCode() }
    $remaining = [Math]::Max(
      1, [int]($settlementDeadline - [DateTime]::UtcNow).TotalMilliseconds
    )
    $captured = [Threading.Tasks.Task]::WaitAll(@($stdoutTask, $stderrTask), $remaining)
    if (-not $captured) { throw "capture-settlement-timeout" }
    if ($null -eq $reason -and
        ($null -ne $stdoutCapture.Overflow -or $null -ne $stderrCapture.Overflow)) {
      $reason = "output-overflow"
    }
    return [ordered]@{
      type = "proof"
      schemaVersion = 1
      requestNonce = $RequestNonce
      sequence = $protocol.Sequence
      settled = $settled
      treeBoundaryVerified = $settled
      activeProcesses = [int]$activeProcesses
      exitCode = $exitCode
      reason = $reason
      teardown = $teardown
      stall = $stall
    }
  } finally {
    if ($null -ne $control) { $control.Dispose() }
    if ($null -ne $stdout) { $stdout.Dispose() }
    if ($null -ne $stderr) { $stderr.Dispose() }
    if ($null -ne $job) { $job.Dispose() }
  }
}

Export-ModuleMember -Function "Invoke-CargoValidationJob"
