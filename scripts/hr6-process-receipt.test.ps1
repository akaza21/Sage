param([Parameter(Mandatory)][string]$ModulePath)

$ErrorActionPreference = "Stop"
. $ModulePath
$process = Start-Process -FilePath "cmd.exe" -ArgumentList "/c", "exit 3" -PassThru
$null = $process.Handle
$process.WaitForExit()
$receipt = Get-ProcessExitReceipt $process
if (-not $receipt.hasExited) { throw "Exited process was not recorded." }
if ($receipt.exitCode -ne 3) { throw "Exit code was not retained." }
if ($null -eq $receipt.exitTime) { throw "Exit time was not retained." }
$withoutExitTime = Get-ProcessExitReceipt ([PSCustomObject]@{
  Id = 1
  HasExited = $true
  ExitCode = 4
  ExitTime = $null
})
if ($withoutExitTime.exitTime -ne $null) { throw "Null exit time was not preserved." }
