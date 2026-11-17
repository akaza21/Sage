param([Parameter(Mandatory)][string]$ModulePath)

$ErrorActionPreference = "Stop"
. $ModulePath
$startInfo = [System.Diagnostics.ProcessStartInfo]::new("cmd.exe", "/c exit 3")
$startInfo.UseShellExecute = $false
$startInfo.CreateNoWindow = $true
$process = [System.Diagnostics.Process]::Start($startInfo)
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
