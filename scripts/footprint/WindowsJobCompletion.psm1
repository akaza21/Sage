Set-StrictMode -Version Latest

function Wait-FootprintJobProcess {
  param(
    [Parameter(Mandatory)]$Process,
    [Parameter(Mandatory)][SageFootprint.NativeJobBoundary]$Boundary,
    [ValidateRange(100, 1500000)][int]$TimeoutMilliseconds = 120000,
    [ValidateRange(100, 5000)][int]$OrphanObservationMilliseconds = 500
  )
  $frequency = [Diagnostics.Stopwatch]::Frequency
  $startedTick = [Diagnostics.Stopwatch]::GetTimestamp()
  $timeoutTicks = [long][Math]::Ceiling(($TimeoutMilliseconds * [double]$frequency) / 1000.0)
  $deadline = $startedTick + $timeoutTicks
  $zeroTick = $null
  try {
    while ([Diagnostics.Stopwatch]::GetTimestamp() -lt $deadline) {
      if ([uint32]$Boundary.ActiveProcesses() -eq 0) {
        $zeroTick = [Diagnostics.Stopwatch]::GetTimestamp()
        break
      }
      Start-Sleep -Milliseconds 10
    }
    if ($null -eq $zeroTick) { throw "Native Job process timed out." }
    $observationTicks = [long][Math]::Ceiling(($OrphanObservationMilliseconds * [double]$frequency) / 1000.0)
    $observationDeadline = $zeroTick + $observationTicks
    while ([Diagnostics.Stopwatch]::GetTimestamp() -lt $observationDeadline) {
      if ([uint32]$Boundary.ActiveProcesses() -ne 0) {
        throw "Native Job became nonempty after reaching zero."
      }
      Start-Sleep -Milliseconds 10
    }
    $Process.Refresh()
    if (-not $Process.HasExited) { throw "Native Job root remained live after zero membership." }
    return [ordered]@{
      exitCode = [int]$Process.ExitCode
      forced = $false
      exited = $true
      treeBoundaryVerified = $true
      zeroDescendantsTick = [long]$zeroTick
      orphanProcessesAfterClose = [int][uint32]$Boundary.ActiveProcesses()
    }
  } catch {
    try { $Boundary.Terminate() } catch {}
    throw
  }
}

Export-ModuleMember -Function "Wait-FootprintJobProcess"
