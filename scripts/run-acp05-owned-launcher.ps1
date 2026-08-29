# ACP-05 collection-only owned launcher. It never adopts or stops ambient Sage.
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$RepoRoot,
  [Parameter(Mandatory = $true)][ValidatePattern('^[a-f0-9]{40}$')][string]$SourceCommit,
  [Parameter(Mandatory = $true)][string]$Executable,
  [Parameter(Mandatory = $true)][ValidatePattern('^[a-f0-9]{64}$')][string]$ExecutableSha256,
  [Parameter(Mandatory = $true)][string]$ListenerExecutable,
  [Parameter(Mandatory = $true)][ValidatePattern('^[a-f0-9]{64}$')][string]$ListenerExecutableSha256,
  [Parameter(Mandatory = $true)][ValidateRange(1024,65535)][int]$CdpPort,
  [Parameter(Mandatory = $true)][string]$EvidenceRoot,
  [ValidateRange(15000,180000)][int]$DeadlineMs = 180000,
  [switch]$ValidateOnly
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if (-not ('SageAcp05.OwnedFileIdentity' -as [type])) { Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
namespace SageAcp05 {
  public static class OwnedFileIdentity {
    [StructLayout(LayoutKind.Sequential)] struct Info {
      public uint Attributes; public System.Runtime.InteropServices.ComTypes.FILETIME Creation, Access, Write;
      public uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
    }
    const uint Read=0x80000000, ShareRead=1, ShareWrite=2, ShareDelete=4, Open=3;
    const uint OpenReparse=0x00200000, Directory=0x10, Reparse=0x400;
    public static string Token(string path) {
      using (SafeFileHandle h=CreateFile(path,Read,ShareRead|ShareWrite|ShareDelete,IntPtr.Zero,Open,OpenReparse,IntPtr.Zero)) {
        if (h.IsInvalid) throw new Win32Exception(Marshal.GetLastWin32Error());
        Info info; if (!GetFileInformationByHandle(h,out info)) throw new Win32Exception(Marshal.GetLastWin32Error());
        if ((info.Attributes & (Directory|Reparse)) != 0) throw new IOException("not a plain file");
        long size=((long)info.SizeHigh<<32)|info.SizeLow;
        return info.Volume.ToString("x8")+":"+info.IndexHigh.ToString("x8")+info.IndexLow.ToString("x8")+":"+size+":"+info.Write.dwHighDateTime.ToString("x8")+info.Write.dwLowDateTime.ToString("x8");
      }
    }
    [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFile(string n,uint a,uint s,IntPtr x,uint c,uint f,IntPtr t);
    [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetFileInformationByHandle(SafeFileHandle h,out Info i);
  }
  public sealed class OwnedFileLease : IDisposable {
    [StructLayout(LayoutKind.Sequential)] struct Info {
      public uint Attributes; public System.Runtime.InteropServices.ComTypes.FILETIME Creation, Access, Write;
      public uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
    }
    const uint Read=0x80000000, ShareRead=1, Open=3, OpenReparse=0x00200000, Directory=0x10, Reparse=0x400;
    SafeFileHandle handle;
    public string FullPath { get; private set; }
    public string Token { get; private set; }
    public string Sha256 { get; private set; }
    public long Length { get; private set; }
    public OwnedFileLease(string path) {
      FullPath=Path.GetFullPath(path);
      handle=CreateFile(FullPath,Read,ShareRead,IntPtr.Zero,Open,OpenReparse,IntPtr.Zero);
      if (handle.IsInvalid) throw new Win32Exception(Marshal.GetLastWin32Error());
      try {
        Info info=ReadInfo();
        if ((info.Attributes & (Directory|Reparse)) != 0) throw new IOException("not a plain file");
        Length=((long)info.SizeHigh<<32)|info.SizeLow;
        Token=FormatToken(info);
        var stream=new FileStream(FullPath,FileMode.Open,FileAccess.Read,FileShare.Read);
        try {
          var sha=System.Security.Cryptography.SHA256.Create();
          try { Sha256=BitConverter.ToString(sha.ComputeHash(stream)).Replace("-","").ToLowerInvariant(); }
          finally { sha.Dispose(); }
        } finally { stream.Dispose(); }
        if (FormatToken(ReadInfo()) != Token) throw new IOException("file changed while lease was acquired");
      } catch { Dispose(); throw; }
    }
    Info ReadInfo() { Info info; if(!GetFileInformationByHandle(handle,out info)) throw new Win32Exception(Marshal.GetLastWin32Error()); return info; }
    static string FormatToken(Info info) { long size=((long)info.SizeHigh<<32)|info.SizeLow; return info.Volume.ToString("x8")+":"+info.IndexHigh.ToString("x8")+info.IndexLow.ToString("x8")+":"+size+":"+info.Write.dwHighDateTime.ToString("x8")+info.Write.dwLowDateTime.ToString("x8"); }
    public string CurrentToken() { return FormatToken(ReadInfo()); }
    public void Dispose() { if(handle!=null){handle.Dispose();handle=null;} GC.SuppressFinalize(this); }
    ~OwnedFileLease(){Dispose();}
    [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFile(string n,uint a,uint s,IntPtr x,uint c,uint f,IntPtr t);
    [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetFileInformationByHandle(SafeFileHandle h,out Info i);
  }
  public static class OwnedDirectoryIdentity {
    [StructLayout(LayoutKind.Sequential)] struct Info {
      public uint Attributes; public System.Runtime.InteropServices.ComTypes.FILETIME Creation, Access, Write;
      public uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
    }
    const uint ShareRead=1,ShareWrite=2,ShareDelete=4,Open=3,OpenReparse=0x00200000,Backup=0x02000000,Directory=0x10,Reparse=0x400;
    public static string Token(string path) {
      using(SafeFileHandle h=CreateFile(path,0,ShareRead|ShareWrite|ShareDelete,IntPtr.Zero,Open,OpenReparse|Backup,IntPtr.Zero)){
        if(h.IsInvalid)throw new Win32Exception(Marshal.GetLastWin32Error());Info i;if(!GetFileInformationByHandle(h,out i))throw new Win32Exception(Marshal.GetLastWin32Error());
        if((i.Attributes&Directory)==0||(i.Attributes&Reparse)!=0)throw new IOException("not a plain directory");return i.Volume.ToString("x8")+":"+i.IndexHigh.ToString("x8")+i.IndexLow.ToString("x8");
      }
    }
    [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFile(string n,uint a,uint s,IntPtr x,uint c,uint f,IntPtr t);
    [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetFileInformationByHandle(SafeFileHandle h,out Info i);
  }
  public sealed class OwnedDirectoryLease : IDisposable {
    const uint ShareRead=1,ShareWrite=2,Open=3,OpenReparse=0x00200000,Backup=0x02000000;
    SafeFileHandle handle;
    public string FullPath { get; private set; }
    public string Token { get; private set; }
    public OwnedDirectoryLease(string path){FullPath=Path.GetFullPath(path);handle=CreateFile(FullPath,0,ShareRead|ShareWrite,IntPtr.Zero,Open,OpenReparse|Backup,IntPtr.Zero);if(handle.IsInvalid)throw new Win32Exception(Marshal.GetLastWin32Error());try{Token=OwnedDirectoryIdentity.Token(FullPath);}catch{Dispose();throw;}}
    public void Dispose(){if(handle!=null){handle.Dispose();handle=null;}GC.SuppressFinalize(this);}~OwnedDirectoryLease(){Dispose();}
    [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFile(string n,uint a,uint s,IntPtr x,uint c,uint f,IntPtr t);
  }
}
'@ }

function Fail([string]$Reason) { throw "acp05-owned: $Reason" }

function Assert-PlainAncestorChain([string]$Path, [string]$Name) {
  if (-not [IO.Path]::IsPathRooted($Path)) { Fail "$Name must be absolute" }
  $full = [IO.Path]::GetFullPath($Path)
  $root = [IO.Path]::GetPathRoot($full)
  $current = $root
  $parts = $full.Substring($root.Length).Split([char[]]@('\','/'), [StringSplitOptions]::RemoveEmptyEntries)
  $candidates = @($root)
  foreach ($part in $parts) { $current = Join-Path $current $part; $candidates += $current }
  foreach ($candidate in $candidates) {
    if (-not (Test-Path -LiteralPath $candidate)) { break }
    $item = Get-Item -LiteralPath $candidate -Force
    if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
      Fail "$Name has a non-plain existing ancestor"
    }
  }
  return $full
}

function PlainDirectory([string]$Path, [string]$Name) {
  $full = Assert-PlainAncestorChain $Path $Name
  if (-not (Test-Path -LiteralPath $full -PathType Container)) { Fail "$Name must be an existing directory" }
  return (Get-Item -LiteralPath $full -Force).FullName
}

function Assert-NewDirectChild([string]$Path, [string]$Parent, [string]$Name) {
  $full = Assert-PlainAncestorChain $Path $Name
  $base = PlainDirectory $Parent "$Name parent"
  if (-not [string]::Equals([IO.Path]::GetDirectoryName($full), $base, [StringComparison]::OrdinalIgnoreCase) -or
      (Test-Path -LiteralPath $full)) { Fail "$Name must be a new direct child" }
  return $full
}

function Get-PlainFileBinding([string]$Path) {
  if (-not [IO.Path]::IsPathRooted($Path) -or -not (Test-Path -LiteralPath $Path -PathType Leaf)) { Fail 'image must be an existing absolute file' }
  [void](Assert-PlainAncestorChain ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($Path))) 'image ancestors')
  $item = Get-Item -LiteralPath $Path -Force
  if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { Fail 'image must be a plain file' }
  $tokenBefore = [SageAcp05.OwnedFileIdentity]::Token($item.FullName)
  $hash = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
  $tokenAfter = [SageAcp05.OwnedFileIdentity]::Token($item.FullName)
  $item = Get-Item -LiteralPath $item.FullName -Force
  if ($tokenBefore -ne $tokenAfter) { Fail 'image changed during identity read' }
  return [ordered]@{ path=$item.FullName; bytes=[long]$item.Length; sha256=$hash; fileToken=$tokenAfter }
}

function Test-ExactFile([string]$Path, [string]$Hash) {
  $binding = Get-PlainFileBinding $Path
  if ([IO.Path]::GetExtension($binding.path) -ne '.exe') { Fail 'executable must be a plain .exe' }
  if ($binding.sha256 -ne $Hash) { Fail 'executable hash mismatch' }
  return $binding
}

function Open-ExactExecutableLease([string]$Path, [string]$Hash, [string]$Name) {
  if (-not [IO.Path]::IsPathRooted($Path) -or [IO.Path]::GetExtension($Path) -ne '.exe') { Fail "$Name must be an absolute .exe" }
  [void](Assert-PlainAncestorChain ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($Path))) "$Name ancestors")
  $lease = [SageAcp05.OwnedFileLease]::new($Path)
  if ($lease.Sha256 -ne $Hash) { $lease.Dispose(); Fail "$Name lease hash mismatch" }
  return $lease
}

function Get-FileLeaseBinding([SageAcp05.OwnedFileLease]$Lease) {
  if ($Lease.CurrentToken() -ne $Lease.Token) { Fail 'leased executable identity changed' }
  return [ordered]@{path=$Lease.FullPath;bytes=[long]$Lease.Length;sha256=$Lease.Sha256;fileToken=$Lease.Token;binding='deny-write-delete-lease'}
}

function Get-NativeDirectoryIdentity([string]$Path) {
  $plain = PlainDirectory $Path 'native directory identity'
  return [ordered]@{path=$plain;token=[SageAcp05.OwnedDirectoryIdentity]::Token($plain)}
}

function Open-DirectoryLease([string]$Path) {
  $plain = PlainDirectory $Path 'directory lease'
  return [SageAcp05.OwnedDirectoryLease]::new($plain)
}

function Assert-DirectoryLease([SageAcp05.OwnedDirectoryLease]$Lease, [string]$Path) {
  $current = Get-NativeDirectoryIdentity $Path
  if (-not [string]::Equals($current.path,$Lease.FullPath,[StringComparison]::OrdinalIgnoreCase) -or $current.token -ne $Lease.Token) { Fail 'directory lease identity changed' }
  return $current
}

function Write-AtomicJson([string]$Path, $Value) {
  $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($Value | ConvertTo-Json -Compress -Depth 10))
  $stream = [IO.FileStream]::new($Path,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None,4096,[IO.FileOptions]::WriteThrough)
  try { $stream.Write($bytes,0,$bytes.Length); $stream.Flush($true) } finally { $stream.Dispose() }
}

function New-EmptyPlainFile([string]$Path) {
  $stream = [IO.FileStream]::new($Path,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
  $stream.Dispose()
}

function New-DeadlineTick([int]$Milliseconds) {
  return [Diagnostics.Stopwatch]::GetTimestamp() + [long][Math]::Floor(($Milliseconds * [double][Diagnostics.Stopwatch]::Frequency) / 1000.0)
}

function Get-RemainingMilliseconds([long]$DeadlineTick) {
  $ticks = $DeadlineTick - [Diagnostics.Stopwatch]::GetTimestamp()
  if ($ticks -le 0) { Fail 'absolute deadline exhausted' }
  $milliseconds = [Math]::Floor(($ticks * 1000.0) / [Diagnostics.Stopwatch]::Frequency)
  if ($milliseconds -lt 1) { Fail 'absolute deadline exhausted' }
  return [int][Math]::Min([int]::MaxValue,$milliseconds)
}

function Wait-Bounded([long]$DeadlineTick, [int]$PollMilliseconds = 100) {
  $remaining = Get-RemainingMilliseconds $DeadlineTick
  Start-Sleep -Milliseconds ([Math]::Min($PollMilliseconds,$remaining))
}

function Assert-BoundedLog([string]$Path, [long]$MaxBytes) {
  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { Fail 'bounded log is missing' }
  $item = Get-Item -LiteralPath $Path -Force
  if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or [long]$item.Length -gt $MaxBytes) { Fail 'bounded log exceeded its contract' }
  return [long]$item.Length
}

function Assert-Acp05LogSet([Collections.IDictionary]$LogLimits) {
  $invalid=$false
  foreach ($entry in $LogLimits.GetEnumerator()) {
    try { Assert-BoundedLog ([string]$entry.Key) ([long]$entry.Value) | Out-Null } catch { $invalid=$true }
  }
  if ($invalid) { Fail 'one or more bounded logs failed their contract' }
  return $true
}

function Assert-FreshCdpPort([int]$Port) {
  $listeners = @([Net.NetworkInformation.IPGlobalProperties]::GetIPGlobalProperties().GetActiveTcpListeners() | Where-Object Port -eq $Port)
  if ($listeners.Count -ne 0) { Fail 'CDP port is already in use' }
  $probe = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,$Port)
  $probe.Server.ExclusiveAddressUse = $true
  try { $probe.Start(1) } catch { Fail 'CDP port cannot be reserved freshly' } finally { $probe.Stop() }
}

function Read-ManualReady([string]$Path, [string]$Nonce) {
  try {
    $before = Get-Item -LiteralPath $Path -Force
    if (($before.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or $before.Length -gt 4096) { return $false }
    $value = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
    $after = Get-Item -LiteralPath $Path -Force
    return $before.Length -eq $after.Length -and $value.schemaVersion -eq 1 -and
      $value.kind -eq 'acp05-owned-manual-ready' -and $value.nonce -eq $Nonce -and
      $value.status -eq 'operator-declared-ready'
  } catch { return $false }
}

function Get-Target([int]$Port, [int]$TimeoutMilliseconds) {
  $response = $null; $stream = $null; $memory = $null
  try {
    $request = [Net.HttpWebRequest]::Create("http://127.0.0.1:$Port/json")
    $request.Proxy = $null; $request.Timeout = $TimeoutMilliseconds; $request.ReadWriteTimeout = $TimeoutMilliseconds
    $response = $request.GetResponse()
    if ($response.ContentLength -gt 65536) { return $null }
    $stream = $response.GetResponseStream(); $memory = [IO.MemoryStream]::new()
    $buffer = [byte[]]::new(4096)
    do {
      $count = $stream.Read($buffer,0,$buffer.Length)
      if ($count -gt 0) { $memory.Write($buffer,0,$count) }
      if ($memory.Length -gt 65536) { return $null }
    } while ($count -gt 0)
    $body = [Text.UTF8Encoding]::new($false,$true).GetString($memory.ToArray()) | ConvertFrom-Json
    $pages = @($body | Where-Object { $_.type -eq 'page' -and $_.title -eq 'Sage' -and
      $_.url -eq 'http://tauri.localhost/' -and $_.webSocketDebuggerUrl -match "^ws://127\.0\.0\.1:$Port/devtools/page/" })
    if ($pages.Count -ne 1) { return $null }
    return $pages[0]
  } catch { return $null } finally {
    if ($null -ne $memory) { $memory.Dispose() }
    if ($null -ne $stream) { $stream.Dispose() }
    if ($null -ne $response) { $response.Dispose() }
  }
}

function Get-OwnedTree([int]$RootProcessId) {
  return @(Get-FootprintDescendants $RootProcessId @(Get-FootprintProcessSnapshot))
}

function Wait-OwnedSageIdentity([int]$RootProcessId, $ExecutableBinding, [long]$DeadlineTick, [string]$Stdout, [string]$Stderr) {
  do {
    Assert-BoundedLog $Stdout 4MB | Out-Null
    Assert-BoundedLog $Stderr 4MB | Out-Null
    $matches = @(Get-OwnedTree $RootProcessId | Where-Object { $_.ExecutablePath -and
      [string]::Equals([IO.Path]::GetFullPath([string]$_.ExecutablePath),$ExecutableBinding.path,[StringComparison]::OrdinalIgnoreCase) })
    if ($matches.Count -gt 1) { Fail 'owned executable identity is ambiguous' }
    if ($matches.Count -eq 1) {
      $identity = ConvertTo-FootprintProcessIdentity $matches[0]
      $process = Get-Process -Id $identity.processId -ErrorAction Stop
      $native = Get-FootprintNativeProcessIdentity $process $ExecutableBinding.path $ExecutableBinding.sha256
      $image = Test-ExactFile $ExecutableBinding.path $ExecutableBinding.sha256
      if ($image.fileToken -ne $ExecutableBinding.fileToken -or $image.bytes -ne $ExecutableBinding.bytes) { Fail 'executable mutated or replaced before identity capture' }
      return [ordered]@{ process=$identity; native=$native; image=$image }
    }
    Wait-Bounded $DeadlineTick
  } while ($true)
}

function Get-OwnedCdpListener([int]$Port, [int]$RootProcessId, $ExpectedImage) {
  $allConnections = @(Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction SilentlyContinue)
  if ($allConnections.Count -eq 0) { return $null }
  $connections = @($allConnections | Where-Object LocalAddress -eq '127.0.0.1')
  if ($allConnections.Count -ne 1 -or $connections.Count -ne 1) { Fail 'CDP listener collided or escaped exact loopback' }
  $ownerPid = [int]$connections[0].OwningProcess
  $matches = @(Get-OwnedTree $RootProcessId | Where-Object { [int]$_.ProcessId -eq $ownerPid })
  if ($matches.Count -ne 1) { Fail 'CDP listener is not in the captured owned descendant tree' }
  $identity = ConvertTo-FootprintProcessIdentity $matches[0]
  if (-not [string]::Equals($identity.executablePath,$ExpectedImage.path,[StringComparison]::OrdinalIgnoreCase)) { Fail 'CDP listener does not use the leased listener executable' }
  $process = Get-Process -Id $identity.processId -ErrorAction Stop
  $native = Get-FootprintNativeProcessIdentity $process $ExpectedImage.path $ExpectedImage.sha256
  return [ordered]@{ process=$identity; native=$native; image=$ExpectedImage; localAddress='127.0.0.1'; port=$Port }
}

function Assert-SameCdpListener($Expected, [int]$Port, [int]$RootProcessId, $ExpectedImage) {
  $actual = Get-OwnedCdpListener $Port $RootProcessId $ExpectedImage
  if ($null -eq $actual) { Fail 'CDP listener disappeared' }
  foreach ($field in @('processId','executablePath','creationDate')) {
    if ($actual.process[$field] -ne $Expected.process[$field]) { Fail 'CDP listener identity changed' }
  }
  foreach ($field in @('path','bytes','sha256','fileToken')) {
    if ($actual.image[$field] -ne $Expected.image[$field]) { Fail 'CDP listener image changed' }
  }
  if ($actual.native.processIdentitySha256 -ne $Expected.native.processIdentitySha256) { Fail 'CDP listener native identity changed' }
  return $actual
}

function Assert-CdpPortReleased([int]$Port) {
  $listeners = @([Net.NetworkInformation.IPGlobalProperties]::GetIPGlobalProperties().GetActiveTcpListeners() | Where-Object Port -eq $Port)
  if ($listeners.Count -ne 0) { Fail 'owned CDP listener survived cleanup' }
}

function Assert-ExecutableNotRunning([string]$Path) {
  $matches = @(Get-FootprintProcessSnapshot | Where-Object { $_.ExecutablePath -and
    [string]::Equals([IO.Path]::GetFullPath([string]$_.ExecutablePath),$Path,[StringComparison]::OrdinalIgnoreCase) })
  if ($matches.Count -ne 0) { Fail 'owned executable survived cleanup' }
}

function New-PowerShellJobStartInfo([string]$WorkingDirectory) {
  $powershell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
  $info = [Diagnostics.ProcessStartInfo]::new()
  $info.FileName = $powershell; $info.WorkingDirectory = $WorkingDirectory; $info.UseShellExecute = $false; $info.Arguments = ''
  return $info
}

function Start-OwnedSageJob($ExecutableBinding, $ListenerBinding, [string]$ProfileRoot, [string]$Stdout, [string]$Stderr, [int]$Port) {
  $runner = @'
$ErrorActionPreference='Stop'
$p=$null
try {
  $p=Start-Process -FilePath $env:SAGE_ACP05_EXE -WorkingDirectory $env:SAGE_ACP05_WORK -RedirectStandardOutput $env:SAGE_ACP05_STDOUT -RedirectStandardError $env:SAGE_ACP05_STDERR -PassThru
  $p.WaitForExit()
  exit $p.ExitCode
} finally {
  if ($null -ne $p) { try { $p.Refresh(); if (-not $p.HasExited) { $p.Kill() } } catch {} }
}
'@
  $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($runner))
  $info = New-PowerShellJobStartInfo ([IO.Path]::GetDirectoryName($ExecutableBinding.path))
  $info.EnvironmentVariables['SAGE_ACP05_EXE']=$ExecutableBinding.path
  $info.EnvironmentVariables['SAGE_ACP05_WORK']=[IO.Path]::GetDirectoryName($ExecutableBinding.path)
  $info.EnvironmentVariables['SAGE_ACP05_STDOUT']=$Stdout
  $info.EnvironmentVariables['SAGE_ACP05_STDERR']=$Stderr
  $info.EnvironmentVariables['SAGE_DISPOSABLE_PROFILE_ROOT']=$ProfileRoot
  $info.EnvironmentVariables.Remove('SAGE_PROJECT_ROOT')
  $info.EnvironmentVariables['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS']="--remote-debugging-address=127.0.0.1 --remote-debugging-port=$Port"
  $info.EnvironmentVariables['WEBVIEW2_BROWSER_EXECUTABLE_FOLDER']=[IO.Path]::GetDirectoryName($ListenerBinding.path)
  return Start-FootprintJobProcess -StartInfo $info -ArgumentList @('-NoProfile','-NonInteractive','-EncodedCommand',$encoded) -ActiveProcessLimit 33
}

function Start-CollectorJob([string]$Node, [string[]]$Arguments, [string]$WorkingDirectory, [string]$Stdout, [string]$Stderr) {
  $runner = @'
$ErrorActionPreference='Stop'
$a=ConvertFrom-Json $env:SAGE_ACP05_COLLECTOR_ARGS
& $env:SAGE_ACP05_NODE @a 1> $env:SAGE_ACP05_COLLECTOR_STDOUT 2> $env:SAGE_ACP05_COLLECTOR_STDERR
exit $LASTEXITCODE
'@
  $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($runner))
  $info = New-PowerShellJobStartInfo $WorkingDirectory
  $info.EnvironmentVariables['SAGE_ACP05_NODE']=$Node
  $info.EnvironmentVariables['SAGE_ACP05_COLLECTOR_ARGS']=($Arguments | ConvertTo-Json -Compress)
  $info.EnvironmentVariables['SAGE_ACP05_COLLECTOR_STDOUT']=$Stdout
  $info.EnvironmentVariables['SAGE_ACP05_COLLECTOR_STDERR']=$Stderr
  return Start-FootprintJobProcess -StartInfo $info -ArgumentList @('-NoProfile','-NonInteractive','-EncodedCommand',$encoded) -ActiveProcessLimit 4
}

function Stop-Acp05OwnedJob($Launch, [long]$DeadlineTick) {
  $remaining = Get-RemainingMilliseconds $DeadlineTick
  if ($remaining -lt 125) { try { $Launch.boundary.Terminate() } finally { $Launch.boundary.Dispose() }; Fail 'cleanup deadline unavailable' }
  $observation = [Math]::Min(500,$remaining-25)
  try {
    $result = Stop-FootprintJobProcess -Process $Launch.process -Boundary $Launch.boundary -GraceMilliseconds 0 -OrphanObservationMilliseconds $observation
    if (-not $result.treeBoundaryVerified -or $result.orphanProcessesAfterClose -ne 0) { Fail 'owned Job cleanup was not terminal-zero' }
    [void](Get-RemainingMilliseconds $DeadlineTick)
    return $result
  } finally { $Launch.boundary.Dispose() }
}

function Stop-Acp05OwnedJobAndCheckLogs($Launch, [long]$DeadlineTick, [Collections.IDictionary]$LogLimits) {
  $cleanup = Stop-Acp05OwnedJob $Launch $DeadlineTick
  [void](Assert-Acp05LogSet $LogLimits)
  [void](Get-RemainingMilliseconds $DeadlineTick)
  return $cleanup
}

function Close-Acp05JobImmediately($Launch) {
  if ($null -eq $Launch) { return }
  try { $Launch.boundary.Terminate() } catch {} finally { try { $Launch.boundary.Dispose() } catch {} }
}

function Get-Sha256Bytes([byte[]]$Bytes) {
  $hasher = [Security.Cryptography.SHA256]::Create()
  try { return ([BitConverter]::ToString($hasher.ComputeHash($Bytes))).Replace('-','').ToLowerInvariant() } finally { $hasher.Dispose() }
}

function Try-WriteMarkerBinding([string]$RawLog, [long]$Offset, [string]$BoundLog, [string]$Nonce, [int]$ProcessId) {
  $stream = [IO.FileStream]::new($RawLog,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite)
  try {
    if ($stream.Length -le $Offset) { return $null }
    $count = $stream.Length-$Offset
    if ($count -gt 4MB) { Fail 'native stdout exceeded its contract' }
    [void]$stream.Seek($Offset,[IO.SeekOrigin]::Begin)
    $bytes = [byte[]]::new([int]$count); $read=0
    while ($read -lt $bytes.Length) { $n=$stream.Read($bytes,$read,$bytes.Length-$read); if($n -eq 0){break}; $read+=$n }
    $text = [Text.UTF8Encoding]::new($false,$true).GetString($bytes,0,$read)
  } finally { $stream.Dispose() }
  foreach ($line in @($text -split "`n")) {
    $clean = $line.TrimEnd("`r")
    if ($clean.Length -le 4096 -and $clean.Contains('ACP renderer recreation pairing passed')) {
      $sourceBytes = [Text.UTF8Encoding]::new($false).GetBytes("$clean`n")
      $attestation = "launcher-attestation: ACP renderer recreation pairing passed nonce=$Nonce pid=$ProcessId source=native-stdout.raw.log`n"
      [IO.File]::AppendAllText($BoundLog,$attestation,[Text.UTF8Encoding]::new($false))
      return [ordered]@{ rawOffset=$Offset; sourceLineSha256=Get-Sha256Bytes $sourceBytes; binding='launcher-attested-owned-stdout' }
    }
  }
  return $null
}

function Test-Acp05CollectorReceipt($Receipt, $MarkerBinding) {
  if ($null -eq $Receipt -or $null -eq $MarkerBinding -or
      $Receipt.outcome -ne 'collected' -or $Receipt.evidenceStatus -ne 'collection-only-incomplete' -or
      $Receipt.marker.proof -ne 'launcher-attested-owned-stdout' -or
      $MarkerBinding.binding -ne 'launcher-attested-owned-stdout') {
    Fail 'collector receipt did not retain truthful incomplete launcher binding'
  }
  return $true
}

function Remove-OwnedStage([string]$Stage, [string]$Parent, [SageAcp05.OwnedDirectoryLease]$ParentLease, $StageIdentity, [string]$ExpectedPrefix) {
  $base = (Assert-DirectoryLease $ParentLease $Parent).path
  $current = Get-NativeDirectoryIdentity $Stage
  $plain = $current.path
  if (-not [string]::Equals([IO.Path]::GetDirectoryName($plain),$base,[StringComparison]::OrdinalIgnoreCase) -or
      -not [string]::Equals($plain,$StageIdentity.path,[StringComparison]::OrdinalIgnoreCase) -or $current.token -ne $StageIdentity.token -or
      -not [IO.Path]::GetFileName($plain).StartsWith($ExpectedPrefix,[StringComparison]::Ordinal)) { Fail 'owned stage identity is invalid' }
  $items = @(Get-ChildItem -LiteralPath $plain -Force -Recurse | Select-Object -First 8193)
  if ($items.Count -gt 8192 -or @($items | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }).Count -ne 0) { Fail 'owned stage cannot be removed safely' }
  Remove-Item -LiteralPath $plain -Recurse -Force
}

function Quarantine-Acp05Stage([string]$Stage, [string]$Parent, [SageAcp05.OwnedDirectoryLease]$ParentLease, $StageIdentity, [string]$Leaf, [string]$Nonce) {
  $base = (Assert-DirectoryLease $ParentLease $Parent).path
  $current = Get-NativeDirectoryIdentity $Stage
  if (-not [string]::Equals([IO.Path]::GetDirectoryName($current.path),$base,[StringComparison]::OrdinalIgnoreCase) -or
      -not [string]::Equals($current.path,$StageIdentity.path,[StringComparison]::OrdinalIgnoreCase) -or $current.token -ne $StageIdentity.token) { Fail 'quarantine source native identity changed' }
  $quarantine = Join-Path $base (".{0}.quarantine-{1}" -f $Leaf,$Nonce)
  [void](Assert-NewDirectChild $quarantine $base 'quarantine directory')
  [IO.Directory]::Move($current.path,$quarantine)
  $renamed = Get-NativeDirectoryIdentity $quarantine
  if ($renamed.token -ne $StageIdentity.token -or (Test-Path -LiteralPath $Stage)) { Fail 'quarantine rename identity was not preserved' }
  [void](Assert-DirectoryLease $ParentLease $Parent)
  return $quarantine
}

function Assert-Acp05CleanupGate([Collections.IDictionary]$Cleanup) {
  foreach ($name in @('collector','sage')) {
    $record = $Cleanup[$name]
    if ($null -eq $record -or -not $record.treeBoundaryVerified -or $record.orphanProcessesAfterClose -ne 0) { Fail 'promotion cleanup gate is not terminal-zero' }
  }
  if (-not $Cleanup.portReleased -or -not $Cleanup.executableAbsent) { Fail 'promotion post-cleanup gate is incomplete' }
  return $true
}

function Publish-Acp05Stage([string]$Stage, [string]$Final, [string]$Parent, [SageAcp05.OwnedDirectoryLease]$ParentLease, $StageIdentity, [Collections.IDictionary]$Cleanup, [Collections.IDictionary]$LogLimits) {
  [void](Assert-Acp05CleanupGate $Cleanup)
  [void](Assert-Acp05LogSet $LogLimits)
  $base = (Assert-DirectoryLease $ParentLease $Parent).path
  $stageNow = Get-NativeDirectoryIdentity $Stage
  if (-not [string]::Equals($stageNow.path,$StageIdentity.path,[StringComparison]::OrdinalIgnoreCase) -or $stageNow.token -ne $StageIdentity.token) { Fail 'staging native identity changed before promotion' }
  $plainStage = $stageNow.path
  $finalPath = Assert-NewDirectChild $Final $base 'final evidence root'
  if (-not [string]::Equals([IO.Path]::GetDirectoryName($plainStage),$base,[StringComparison]::OrdinalIgnoreCase) -or
      [IO.Path]::GetPathRoot($plainStage) -ne [IO.Path]::GetPathRoot($finalPath)) { Fail 'staging promotion must stay on one volume and parent' }
  $items = @(Get-ChildItem -LiteralPath $plainStage -Force -Recurse | Select-Object -First 8193)
  if ($items.Count -gt 8192 -or @($items | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }).Count -ne 0) { Fail 'staging inventory is invalid' }
  Move-Item -LiteralPath $plainStage -Destination $finalPath
  $promoted = Get-NativeDirectoryIdentity $finalPath
  if ($promoted.token -ne $StageIdentity.token -or (Test-Path -LiteralPath $plainStage)) { Fail 'staging promotion identity was not preserved' }
  [void](Assert-DirectoryLease $ParentLease $Parent)
  return $finalPath
}

Import-Module (Join-Path $PSScriptRoot 'footprint/ProcessIdentity.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'footprint/DisposableProfile.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'footprint/WindowsJob.psm1') -Force

$repo = PlainDirectory $RepoRoot 'repo root'
$expectedHead = (& git -C $repo rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $expectedHead -ne $SourceCommit) { Fail 'repo HEAD does not equal source commit' }
if ((& git -C $repo status --porcelain=v1 --untracked-files=all)) { Fail 'repo must be clean' }
$localEvidencePath = Join-Path $repo 'local-evidence'
[void](Assert-PlainAncestorChain $localEvidencePath 'local-evidence')
if (-not (Test-Path -LiteralPath $localEvidencePath)) { [void](New-Item -ItemType Directory -Path $localEvidencePath) }
$localEvidence = PlainDirectory $localEvidencePath 'local-evidence'
$finalEvidence = Assert-NewDirectChild $EvidenceRoot $localEvidence 'evidence root'
if (Test-Path -LiteralPath "$finalEvidence.failure.json") { Fail 'failure receipt already exists' }
$before = Test-ExactFile $Executable $ExecutableSha256
$preflightSage = $before
$listenerPreflight = Test-ExactFile $ListenerExecutable $ListenerExecutableSha256
$existing = @(Get-FootprintProcessSnapshot | Where-Object { $_.ExecutablePath -and [string]::Equals([IO.Path]::GetFullPath([string]$_.ExecutablePath),$before.path,[StringComparison]::OrdinalIgnoreCase) })
if ($existing.Count -ne 0) { Fail 'exact executable is already running; ambient process not adopted' }
Assert-FreshCdpPort $CdpPort
if ($ValidateOnly) {
  $sageProbe=$null; $listenerProbe=$null
  try {
    $sageProbe=Open-ExactExecutableLease $Executable $ExecutableSha256 'executable'
    $listenerProbe=Open-ExactExecutableLease $ListenerExecutable $ListenerExecutableSha256 'listener executable'
    Write-Output ([ordered]@{executable=Get-FileLeaseBinding $sageProbe;listenerExecutable=Get-FileLeaseBinding $listenerProbe} | ConvertTo-Json -Compress -Depth 4)
  } finally { if($null-ne $listenerProbe){$listenerProbe.Dispose()};if($null-ne $sageProbe){$sageProbe.Dispose()} }
  exit 0
}

$deadlineTick = New-DeadlineTick $DeadlineMs
$sageFileLease=$null; $listenerFileLease=$null; $evidenceParentLease=$null
try {
  $sageFileLease=Open-ExactExecutableLease $Executable $ExecutableSha256 'executable'
  $listenerFileLease=Open-ExactExecutableLease $ListenerExecutable $ListenerExecutableSha256 'listener executable'
  $before=Get-FileLeaseBinding $sageFileLease
  $listenerBinding=Get-FileLeaseBinding $listenerFileLease
  if ($before.fileToken -ne $preflightSage.fileToken -or $listenerBinding.fileToken -ne $listenerPreflight.fileToken) { Fail 'executable changed between preflight and lease' }
  $evidenceParentLease=Open-DirectoryLease $localEvidence
} catch {
  if($null-ne $listenerFileLease){$listenerFileLease.Dispose()};if($null-ne $sageFileLease){$sageFileLease.Dispose()};if($null-ne $evidenceParentLease){$evidenceParentLease.Dispose()}
  throw
}
$nonce = [Guid]::NewGuid().ToString('N')
$stagePrefix = ".{0}.staging-" -f [IO.Path]::GetFileName($finalEvidence)
$stage = Join-Path $localEvidence ($stagePrefix+$nonce)
[void](Assert-NewDirectChild $stage $localEvidence 'staging directory')
[void](New-Item -ItemType Directory -Path $stage)
$stage = PlainDirectory $stage 'staging directory'
$stageIdentity = Get-NativeDirectoryIdentity $stage
$checkpoint = Join-Path $stage 'manual-ready.json'
$rawStdout = Join-Path $stage 'native-stdout.raw.log'
$rawStderr = Join-Path $stage 'native-stderr.raw.log'
$boundMarker = Join-Path $stage 'native-marker.bound.log'
$collectorStdout = Join-Path $stage 'collector-stdout.log'
$collectorStderr = Join-Path $stage 'collector-stderr.log'
$collectorReceipt = Join-Path $stage 'acp05-receipt.json'
$launcherReceipt = Join-Path $stage 'acp05-owned-launcher-receipt.json'
$allLogLimits = [ordered]@{$rawStdout=4MB;$rawStderr=4MB;$boundMarker=4096;$collectorStdout=64KB;$collectorStderr=64KB}
$profile=$null; $sageLaunch=$null; $collectorLaunch=$null; $sageIdentity=$null; $listenerBefore=$null
$sageCleanup=$null; $collectorCleanup=$null; $markerBinding=$null; $failureKind=$null; $phase='launch'; $collected=$false

try {
  Write-AtomicJson $checkpoint ([ordered]@{schemaVersion=1;kind='acp05-owned-manual-ready';nonce=$nonce;status='pending';collectionOnly=$true})
  foreach ($path in @($rawStdout,$rawStderr,$boundMarker,$collectorStdout,$collectorStderr)) { New-EmptyPlainFile $path }
  Write-Output "ACP-05 manual-ready checkpoint: $checkpoint"
  $profile = New-C0BDisposableProfile $stage 'owned-profile'
  $phase='port-collision'; Assert-FreshCdpPort $CdpPort
  $phase='launch'; $sageLaunch = Start-OwnedSageJob $before $listenerBinding $profile.root $rawStdout $rawStderr $CdpPort
  $phase='owned-identity'; $sageIdentity = Wait-OwnedSageIdentity ([int]$sageLaunch.process.Id) $before $deadlineTick $rawStdout $rawStderr
  do {
    Assert-BoundedLog $rawStdout 4MB | Out-Null; Assert-BoundedLog $rawStderr 4MB | Out-Null
    $listenerBefore = Get-OwnedCdpListener $CdpPort ([int]$sageIdentity.process.processId) $listenerBinding
    if ($null -ne $listenerBefore) { break }
    Wait-Bounded $deadlineTick
  } while ($true)
  $phase='manual-ready'
  do {
    Assert-BoundedLog $rawStdout 4MB | Out-Null; Assert-BoundedLog $rawStderr 4MB | Out-Null
    $sageLaunch.process.Refresh(); if ($sageLaunch.process.HasExited) { Fail 'owned launcher root exited before admission' }
    if (Read-ManualReady $checkpoint $nonce) {
      $target = Get-Target $CdpPort ([Math]::Min(1000,(Get-RemainingMilliseconds $deadlineTick)))
      if ($null -ne $target) { break }
    }
    Wait-Bounded $deadlineTick
  } while ($true)
  $phase='listener-collision'; [void](Assert-SameCdpListener $listenerBefore $CdpPort ([int]$sageIdentity.process.processId) $listenerBinding)
  $phase='collector-budget'
  $remaining = Get-RemainingMilliseconds $deadlineTick
  $cleanupReserve = 1500
  if ($remaining-$cleanupReserve -lt 1000) { Fail 'collector minimum deadline is unavailable' }
  $collectorDeadline = [int]($remaining-$cleanupReserve)
  $rawOffset = Assert-BoundedLog $rawStdout 4MB
  $node = (Get-Command node.exe -ErrorAction Stop).Source
  $collectorScript = Join-Path $PSScriptRoot 'acp05-native-reload-capture.mjs'
  $collectorArgs = @($collectorScript,'--repo-root',$repo,'--source-commit',$SourceCommit,'--executable',$before.path,
    '--executable-sha256',$before.sha256,'--cdp-endpoint',"http://127.0.0.1:$CdpPort",'--evidence-root',$stage,
    '--marker-log',$boundMarker,'--marker-proof','launcher-attested-owned-stdout','--marker-nonce',$nonce,'--process-id',"$($sageIdentity.process.processId)",
    '--receipt',$collectorReceipt,'--deadline-ms',"$collectorDeadline")
  $phase='collector'; $collectorLaunch = Start-CollectorJob $node $collectorArgs $repo $collectorStdout $collectorStderr
  do {
    Assert-BoundedLog $rawStdout 4MB | Out-Null; Assert-BoundedLog $rawStderr 4MB | Out-Null
    Assert-BoundedLog $collectorStdout 64KB | Out-Null; Assert-BoundedLog $collectorStderr 64KB | Out-Null
    if ($null -eq $markerBinding) { $markerBinding = Try-WriteMarkerBinding $rawStdout $rawOffset $boundMarker $nonce ([int]$sageIdentity.process.processId) }
    $collectorLaunch.process.Refresh()
    if ($collectorLaunch.process.HasExited) { break }
    Wait-Bounded $deadlineTick
  } while ($true)
  if ($null -eq $markerBinding) { $markerBinding = Try-WriteMarkerBinding $rawStdout $rawOffset $boundMarker $nonce ([int]$sageIdentity.process.processId) }
  $collectorExit = $collectorLaunch.process.ExitCode
  $phase='collector-cleanup'; $collectorCleanup=Stop-Acp05OwnedJobAndCheckLogs $collectorLaunch $deadlineTick $allLogLimits; $collectorLaunch=$null
  if ($collectorExit -ne 0 -or -not (Test-Path -LiteralPath $collectorReceipt -PathType Leaf)) { Fail 'collector failed' }
  $receipt = Get-Content -LiteralPath $collectorReceipt -Raw | ConvertFrom-Json
  [void](Test-Acp05CollectorReceipt $receipt $markerBinding)
  $phase='postvalidation'
  [void](Assert-SameCdpListener $listenerBefore $CdpPort ([int]$sageIdentity.process.processId) $listenerBinding)
  $after = Get-FileLeaseBinding $sageFileLease
  [void](Get-FileLeaseBinding $listenerFileLease)
  if ($after.bytes -ne $before.bytes -or $after.fileToken -ne $before.fileToken) { Fail 'executable mutated or replaced during collection' }
  $collected=$true
} catch {
  $failureKind=$phase
} finally {
  if ($null -ne $collectorLaunch) {
    try { $collectorCleanup=Stop-Acp05OwnedJobAndCheckLogs $collectorLaunch $deadlineTick $allLogLimits; $collectorLaunch=$null } catch { Close-Acp05JobImmediately $collectorLaunch; $collectorLaunch=$null; $failureKind='cleanup-failed' }
  }
  if ($null -ne $sageLaunch) {
    try { $sageCleanup=Stop-Acp05OwnedJobAndCheckLogs $sageLaunch $deadlineTick $allLogLimits; $sageLaunch=$null } catch { Close-Acp05JobImmediately $sageLaunch; $sageLaunch=$null; $failureKind='cleanup-failed' }
  }
  if ($null -ne $profile) {
    try { Remove-C0BDisposableProfile $stage $profile.root; $profile=$null } catch { $failureKind='cleanup-failed' }
  }
}

if ($collected -and -not $failureKind) {
  try {
    [void](Get-RemainingMilliseconds $deadlineTick)
    Assert-CdpPortReleased $CdpPort
    Assert-ExecutableNotRunning $before.path
    [void](Get-FileLeaseBinding $sageFileLease)
    [void](Get-FileLeaseBinding $listenerFileLease)
    [void](Assert-Acp05LogSet $allLogLimits)
    [void](Get-RemainingMilliseconds $deadlineTick)
    $cleanupState=[ordered]@{collector=$collectorCleanup;sage=$sageCleanup;portReleased=$true;executableAbsent=$true}
    Write-AtomicJson $launcherReceipt ([ordered]@{schemaVersion=3;kind='acp05-owned-launcher';outcome='collected';collectionOnly=$true;evidenceStatus='collection-only-incomplete';
      sourceCommit=$SourceCommit;executable=$before;ownedProcess=$sageIdentity;cdpListener=$listenerBefore;
      manualReady='operator-declared-unverified';markerBinding=$markerBinding;markerProof='launcher-attested-owned-stdout';
      nativeMarkerSatisfied=$false;remainingForW1_3='native marker emitted by the owned production process';collectorReceipt='acp05-receipt.json';cleanup=$cleanupState})
    [void](Assert-Acp05LogSet $allLogLimits)
    $published = Publish-Acp05Stage $stage $finalEvidence $localEvidence $evidenceParentLease $stageIdentity $cleanupState $allLogLimits
    $evidenceParentLease.Dispose();$evidenceParentLease=$null;$listenerFileLease.Dispose();$listenerFileLease=$null;$sageFileLease.Dispose();$sageFileLease=$null
    Write-Output (Join-Path $published 'acp05-owned-launcher-receipt.json')
    exit 0
  } catch { $failureKind='promotion-failed' }
}

if (Test-Path -LiteralPath $stage -PathType Container) {
  try { Remove-OwnedStage $stage $localEvidence $evidenceParentLease $stageIdentity $stagePrefix }
  catch {
    $failureKind='quarantine-required'
    try { [void](Quarantine-Acp05Stage $stage $localEvidence $evidenceParentLease $stageIdentity ([IO.Path]::GetFileName($finalEvidence)) $nonce) }
    catch { $failureKind='quarantine-hard-failure' }
  }
}
if($null-ne $evidenceParentLease){$evidenceParentLease.Dispose();$evidenceParentLease=$null};if($null-ne $listenerFileLease){$listenerFileLease.Dispose();$listenerFileLease=$null};if($null-ne $sageFileLease){$sageFileLease.Dispose();$sageFileLease=$null}
$failurePath = "$finalEvidence.failure.json"
if (-not (Test-Path -LiteralPath $failurePath)) {
  Write-AtomicJson $failurePath ([ordered]@{schemaVersion=1;kind='acp05-owned-launcher-failure';outcome='failed';collectionOnly=$true;category=$failureKind})
}
[Console]::Error.WriteLine("acp05-owned: failed ($failureKind)")
exit 1
