param(
  [Parameter(Mandatory)][ValidatePattern('^[a-z0-9][a-z0-9-]{0,63}$')][string]$CampaignId,
  [Parameter(Mandatory)][string]$AuthenticatedSourceRoot,
  [Parameter(Mandatory)][string]$PowerShellPath,
  [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{64}$')][string]$PowerShellSha256,
  [Parameter(Mandatory)][ValidateRange(1, 536870912)][long]$PowerShellBytes,
  [Parameter(Mandatory)][ValidateLength(1, 262144)][string]$CampaignBootstrapBase64,
  [Parameter(Mandatory)][ValidateLength(1, 16384)][string]$LockedLoaderEncodedCommand,
  [Parameter(Mandatory)][ValidateRange(100, 1500000)][int]$OuterJobTimeoutMilliseconds
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if (-not ("SageInstalledSmoke.ControllerBoundedFile" -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
namespace SageInstalledSmoke {
  public static class ControllerBoundedFile {
    const uint GENERIC_READ = 0x80000000, SHARE_READ = 1, OPEN_EXISTING = 3;
    const uint OPEN_REPARSE_POINT = 0x00200000, SEQUENTIAL_SCAN = 0x08000000;
    const uint DIRECTORY = 0x10, REPARSE_POINT = 0x400;
    [StructLayout(LayoutKind.Sequential)]
    struct FileInformation {
      public uint Attributes;
      public System.Runtime.InteropServices.ComTypes.FILETIME CreationTime;
      public System.Runtime.InteropServices.ComTypes.FILETIME LastAccessTime;
      public System.Runtime.InteropServices.ComTypes.FILETIME LastWriteTime;
      public uint VolumeSerialNumber, FileSizeHigh, FileSizeLow, NumberOfLinks, FileIndexHigh, FileIndexLow;
    }
    public static byte[] Read(string path, int maximumBytes) {
      using (SafeFileHandle handle = CreateFile(path, GENERIC_READ, SHARE_READ, IntPtr.Zero,
          OPEN_EXISTING, OPEN_REPARSE_POINT | SEQUENTIAL_SCAN, IntPtr.Zero)) {
        if (handle.IsInvalid) throw new Win32Exception(Marshal.GetLastWin32Error());
        FileInformation info;
        if (!GetFileInformationByHandle(handle, out info))
          throw new Win32Exception(Marshal.GetLastWin32Error());
        if ((info.Attributes & (DIRECTORY | REPARSE_POINT)) != 0)
          throw new IOException("Controller input is not a plain file.");
        long length = ((long)info.FileSizeHigh << 32) | info.FileSizeLow;
        if (length < 1 || length > maximumBytes)
          throw new IOException("Controller input exceeds its bound.");
        byte[] bytes = new byte[(int)length];
        using (var stream = new FileStream(handle, FileAccess.Read, 4096, false)) {
          int offset = 0;
          while (offset < bytes.Length) {
            int read = stream.Read(bytes, offset, bytes.Length - offset);
            if (read == 0) throw new EndOfStreamException();
            offset += read;
          }
          if (stream.ReadByte() != -1) throw new IOException("Controller input changed size.");
        }
        return bytes;
      }
    }
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern SafeFileHandle CreateFile(string name, uint access, uint share, IntPtr security,
      uint creation, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool GetFileInformationByHandle(SafeFileHandle handle, out FileInformation info);
  }
}
'@
}

function Test-ControllerFileIdentity {
  param([string]$Path, [long]$Bytes, [string]$Sha256)
  try {
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    return -not $item.PSIsContainer -and
      -not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -and
      $item.Length -eq $Bytes -and
      (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() -eq $Sha256
  } catch { return $false }
}

function Test-ControllerFixedTimeHex {
  param([string]$Left, [string]$Right)
  if ($Left -notmatch '^[0-9a-f]{64}$' -or $Right -notmatch '^[0-9a-f]{64}$') { return $false }
  $difference = 0
  for ($index = 0; $index -lt 64; $index++) {
    $difference = $difference -bor ([int][char]$Left[$index] -bxor [int][char]$Right[$index])
  }
  return $difference -eq 0
}

$job = $null
$proof = [ordered]@{
  schemaVersion = 1
  campaignId = $CampaignId
  status = "control-failed"
  jobConverged = $false
  childExitCode = $null
  forced = $false
  orphanProcessesAfterClose = $null
  receiptSha256 = $null
  receiptAuthenticationSha256 = $null
}
try {
  if (-not (Test-ControllerFileIdentity $PowerShellPath $PowerShellBytes $PowerShellSha256)) {
    throw "Authenticated PowerShell identity changed before campaign Job launch."
  }
  Import-Module (Join-Path $AuthenticatedSourceRoot "scripts/footprint/WindowsJob.psm1") -Force
  Import-Module (Join-Path $AuthenticatedSourceRoot "scripts/footprint/WindowsJobCompletion.psm1") -Force
  $startInfo = [Diagnostics.ProcessStartInfo]::new()
  $startInfo.FileName = [IO.Path]::GetFullPath($PowerShellPath)
  $startInfo.WorkingDirectory = [IO.Path]::GetFullPath($AuthenticatedSourceRoot)
  $startInfo.UseShellExecute = $false
  $startInfo.CreateNoWindow = $true
  $startInfo.EnvironmentVariables["SAGE_INSTALLED_SMOKE_BOOTSTRAP"] = $CampaignBootstrapBase64
  $job = Start-FootprintJobProcess $startInfo 33 @(
    "-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
    "-EncodedCommand", $LockedLoaderEncodedCommand
  )
  try {
    $completion = Wait-FootprintJobProcess $job.process $job.boundary $OuterJobTimeoutMilliseconds 500
    $proof.status = if ($completion.exitCode -eq 0) { "completed" } else { "child-exit" }
    $proof.jobConverged = [bool]$completion.treeBoundaryVerified
    $proof.childExitCode = [int]$completion.exitCode
    $proof.forced = [bool]$completion.forced
    $proof.orphanProcessesAfterClose = [int]$completion.orphanProcessesAfterClose
  } catch {
    $completion = Stop-FootprintJobProcess $job.process $job.boundary 0 1000
    $proof.status = if ($_.Exception.Message -match 'timed out') { "timeout" } else { "child-failed" }
    $proof.jobConverged = [bool]$completion.treeBoundaryVerified
    $proof.forced = $true
    $proof.orphanProcessesAfterClose = [int]$completion.orphanProcessesAfterClose
  }
} catch {
  $proof.status = "control-failed"
} finally {
  if ($null -ne $job) {
    try { $job.process.Dispose() } catch {}
    try { $job.boundary.Dispose() } catch {}
  }
}
if ($proof.jobConverged) {
  try {
    $campaignRequest = [Text.Encoding]::UTF8.GetString(
      [Convert]::FromBase64String($CampaignBootstrapBase64)
    ) | ConvertFrom-Json
    $output = [string]$campaignRequest.parameters.OutputDirectory
    [byte[]]$receiptBytes = [SageInstalledSmoke.ControllerBoundedFile]::Read((Join-Path $output "receipt.json"), 262144)
    [byte[]]$authenticationBytes = [SageInstalledSmoke.ControllerBoundedFile]::Read((Join-Path $output "receipt-authentication.json"), 1024)
    $authentication = [Text.UTF8Encoding]::new($false, $true).GetString($authenticationBytes) | ConvertFrom-Json
    $authenticationKeys = @($authentication.PSObject.Properties.Name | Sort-Object)
    if (($authenticationKeys -join "`0") -ne (@("authenticationSha256", "campaignId", "receiptSha256", "schemaVersion") -join "`0") -or
        [int]$authentication.schemaVersion -ne 1 -or [string]$authentication.campaignId -ne $CampaignId) {
      throw "Campaign receipt authentication record is invalid."
    }
    $hasher = [Security.Cryptography.SHA256]::Create()
    try { $receiptSha256 = [BitConverter]::ToString($hasher.ComputeHash($receiptBytes)).Replace('-', '').ToLowerInvariant() }
    finally { $hasher.Dispose() }
    [byte[]]$cleanupKey = [Convert]::FromBase64String([string]$campaignRequest.parameters.CleanupKeyBase64)
    if ($cleanupKey.Length -ne 32) { throw "Campaign receipt authentication key is invalid." }
    $mac = [Security.Cryptography.HMACSHA256]::new($cleanupKey)
    try { $receiptAuthenticationSha256 = [BitConverter]::ToString($mac.ComputeHash($receiptBytes)).Replace('-', '').ToLowerInvariant() }
    finally { $mac.Dispose() }
    if (-not (Test-ControllerFixedTimeHex $receiptSha256 ([string]$authentication.receiptSha256)) -or
        -not (Test-ControllerFixedTimeHex $receiptAuthenticationSha256 ([string]$authentication.authenticationSha256))) {
      throw "Campaign receipt authentication changed."
    }
    $proof.receiptSha256 = $receiptSha256
    $proof.receiptAuthenticationSha256 = $receiptAuthenticationSha256
  } catch {
    $proof.receiptSha256 = $null
    $proof.receiptAuthenticationSha256 = $null
  }
}
[Console]::Out.WriteLine(($proof | ConvertTo-Json -Compress))
