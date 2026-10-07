function Get-ProcessExitReceipt($Process) {
  if ($null -eq $Process) { return $null }
  $hasExited = $Process.HasExited
  $exitTime = if ($hasExited) { $Process.ExitTime } else { $null }
  return [ordered]@{
    pid = $Process.Id
    hasExited = $hasExited
    exitCode = if ($hasExited) { $Process.ExitCode } else { $null }
    exitTime = if ($null -ne $exitTime) { $exitTime.ToUniversalTime().ToString("o") } else { $null }
  }
}
