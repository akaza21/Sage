Set-StrictMode -Version Latest

if (-not ("SageFootprint.NativeJobBoundary" -as [type])) {
  Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;

namespace SageFootprint {
  public sealed class NativeJobBoundary : IDisposable {
    const uint CREATE_SUSPENDED = 0x00000004;
    const uint CREATE_UNICODE_ENVIRONMENT = 0x00000400;
    const uint JOB_OBJECT_LIMIT_ACTIVE_PROCESS = 0x00000008;
    const uint JOB_OBJECT_LIMIT_BREAKAWAY_OK = 0x00000800;
    const uint JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK = 0x00001000;
    const uint JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000;
    const int JobObjectBasicAccountingInformation = 1;
    const int JobObjectExtendedLimitInformation = 9;

    IntPtr job;
    public int ProcessId { get; private set; }

    NativeJobBoundary(IntPtr handle) { job = handle; }

    public static NativeJobBoundary Start(
      string executable,
      string[] arguments,
      string workingDirectory,
      string[] environmentEntries,
      uint activeProcessLimit
    ) {
      if (String.IsNullOrWhiteSpace(executable) || arguments == null ||
          arguments.Length > 32 ||
          String.IsNullOrWhiteSpace(workingDirectory) ||
          executable.IndexOf('\0') >= 0 || executable.IndexOf('"') >= 0 ||
          activeProcessLimit == 0 || activeProcessLimit > 33) {
        throw new ArgumentException("Invalid native Job launch request.");
      }
      IntPtr job = CreateJobObject(IntPtr.Zero, null);
      if (job == IntPtr.Zero) throw NativeFailure("CreateJobObject");
      var boundary = new NativeJobBoundary(job);
      PROCESS_INFORMATION process = new PROCESS_INFORMATION();
      IntPtr environment = IntPtr.Zero;
      bool resumed = false;
      try {
        boundary.Configure(activeProcessLimit);
        environment = Marshal.StringToHGlobalUni(EnvironmentBlock(environmentEntries));
        var startup = new STARTUPINFO();
        startup.cb = Marshal.SizeOf(typeof(STARTUPINFO));
        var commandLine = new StringBuilder(BuildCommandLine(executable, arguments));
        if (!CreateProcess(
          executable,
          commandLine,
          IntPtr.Zero,
          IntPtr.Zero,
          false,
          CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
          environment,
          workingDirectory,
          ref startup,
          out process
        )) throw NativeFailure("CreateProcessW");
        if (!AssignProcessToJobObject(job, process.hProcess))
          throw NativeFailure("AssignProcessToJobObject");
        boundary.ProcessId = checked((int)process.dwProcessId);
        if (ResumeThread(process.hThread) == UInt32.MaxValue)
          throw NativeFailure("ResumeThread");
        resumed = true;
        return boundary;
      } catch {
        if (process.hProcess != IntPtr.Zero) TerminateJobObject(job, 1);
        boundary.Dispose();
        throw;
      } finally {
        if (environment != IntPtr.Zero) Marshal.FreeHGlobal(environment);
        if (process.hThread != IntPtr.Zero) CloseHandle(process.hThread);
        if (process.hProcess != IntPtr.Zero) CloseHandle(process.hProcess);
        if (!resumed) boundary.ProcessId = 0;
      }
    }

    public static string QuoteArgumentForTest(string argument) {
      return QuoteArgument(argument);
    }

    static string BuildCommandLine(string executable, string[] arguments) {
      var command = new StringBuilder(QuoteArgument(executable));
      foreach (string argument in arguments) {
        command.Append(' ').Append(QuoteArgument(argument));
        // CreateProcessW's 32,767-character limit includes the terminating NUL.
        if (command.Length >= 32767)
          throw new ArgumentException("Native Job command line exceeds its bound.");
      }
      return command.ToString();
    }

    static string QuoteArgument(string argument) {
      if (argument == null || argument.IndexOf('\0') >= 0 || argument.Length > 16384)
        throw new ArgumentException("Invalid native Job argument.");
      if (argument.Length > 0 && argument.IndexOfAny(new [] {' ', '\t', '\r', '\n', '\v', '\f', '"'}) < 0)
        return argument;
      var quoted = new StringBuilder("\"");
      int slashes = 0;
      foreach (char value in argument) {
        if (value == '\\') { slashes++; continue; }
        if (value == '"') quoted.Append('\\', slashes * 2 + 1);
        else quoted.Append('\\', slashes);
        quoted.Append(value);
        slashes = 0;
      }
      quoted.Append('\\', slashes * 2).Append('"');
      return quoted.ToString();
    }

    public uint ActiveProcesses() {
      EnsureOpen();
      var accounting = new JOBOBJECT_BASIC_ACCOUNTING_INFORMATION();
      uint returned;
      if (!QueryInformationJobObjectAccounting(
        job,
        JobObjectBasicAccountingInformation,
        ref accounting,
        (uint)Marshal.SizeOf(typeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION)),
        out returned
      )) throw NativeFailure("QueryInformationJobObject");
      return accounting.ActiveProcesses;
    }

    public void Terminate() {
      EnsureOpen();
      if (!TerminateJobObject(job, 1)) throw NativeFailure("TerminateJobObject");
    }

    void Configure(uint activeProcessLimit) {
      var limits = new JOBOBJECT_EXTENDED_LIMIT_INFORMATION();
      limits.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
      limits.BasicLimitInformation.ActiveProcessLimit = activeProcessLimit;
      if (!SetInformationJobObject(
        job,
        JobObjectExtendedLimitInformation,
        ref limits,
        (uint)Marshal.SizeOf(typeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION))
      )) throw NativeFailure("SetInformationJobObject");
      var actual = new JOBOBJECT_EXTENDED_LIMIT_INFORMATION();
      uint returned;
      if (!QueryInformationJobObjectLimits(
        job,
        JobObjectExtendedLimitInformation,
        ref actual,
        (uint)Marshal.SizeOf(typeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION)),
        out returned
      )) throw NativeFailure("QueryInformationJobObject");
      uint flags = actual.BasicLimitInformation.LimitFlags;
      uint forbidden = JOB_OBJECT_LIMIT_BREAKAWAY_OK | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK;
      if ((flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE) == 0 ||
          (flags & JOB_OBJECT_LIMIT_ACTIVE_PROCESS) == 0 ||
          (flags & forbidden) != 0 ||
          actual.BasicLimitInformation.ActiveProcessLimit != activeProcessLimit) {
        throw new InvalidOperationException("Native Job limits were not applied exactly.");
      }
    }

    static string EnvironmentBlock(string[] entries) {
      if (entries == null) throw new ArgumentNullException("environmentEntries");
      var ordered = entries.OrderBy(value => value, StringComparer.OrdinalIgnoreCase).ToArray();
      foreach (string entry in ordered) {
        if (String.IsNullOrEmpty(entry) || entry.IndexOf('\0') >= 0 || entry.IndexOf('=') <= 0)
          throw new ArgumentException("Invalid environment entry.");
      }
      return String.Join("\0", ordered) + "\0\0";
    }

    void EnsureOpen() {
      if (job == IntPtr.Zero) throw new ObjectDisposedException("NativeJobBoundary");
    }

    static Win32Exception NativeFailure(string operation) {
      return new Win32Exception(Marshal.GetLastWin32Error(), operation + " failed.");
    }

    public void Dispose() {
      IntPtr handle = job;
      job = IntPtr.Zero;
      if (handle != IntPtr.Zero) CloseHandle(handle);
      GC.SuppressFinalize(this);
    }

    ~NativeJobBoundary() { Dispose(); }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct STARTUPINFO {
      public int cb;
      public string lpReserved;
      public string lpDesktop;
      public string lpTitle;
      public uint dwX;
      public uint dwY;
      public uint dwXSize;
      public uint dwYSize;
      public uint dwXCountChars;
      public uint dwYCountChars;
      public uint dwFillAttribute;
      public uint dwFlags;
      public ushort wShowWindow;
      public ushort cbReserved2;
      public IntPtr lpReserved2;
      public IntPtr hStdInput;
      public IntPtr hStdOutput;
      public IntPtr hStdError;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct PROCESS_INFORMATION {
      public IntPtr hProcess;
      public IntPtr hThread;
      public uint dwProcessId;
      public uint dwThreadId;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct JOBOBJECT_BASIC_LIMIT_INFORMATION {
      public long PerProcessUserTimeLimit;
      public long PerJobUserTimeLimit;
      public uint LimitFlags;
      public UIntPtr MinimumWorkingSetSize;
      public UIntPtr MaximumWorkingSetSize;
      public uint ActiveProcessLimit;
      public UIntPtr Affinity;
      public uint PriorityClass;
      public uint SchedulingClass;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct IO_COUNTERS {
      public ulong ReadOperationCount;
      public ulong WriteOperationCount;
      public ulong OtherOperationCount;
      public ulong ReadTransferCount;
      public ulong WriteTransferCount;
      public ulong OtherTransferCount;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
      public JOBOBJECT_BASIC_LIMIT_INFORMATION BasicLimitInformation;
      public IO_COUNTERS IoInfo;
      public UIntPtr ProcessMemoryLimit;
      public UIntPtr JobMemoryLimit;
      public UIntPtr PeakProcessMemoryUsed;
      public UIntPtr PeakJobMemoryUsed;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct JOBOBJECT_BASIC_ACCOUNTING_INFORMATION {
      public long TotalUserTime;
      public long TotalKernelTime;
      public long ThisPeriodTotalUserTime;
      public long ThisPeriodTotalKernelTime;
      public uint TotalPageFaultCount;
      public uint TotalProcesses;
      public uint ActiveProcesses;
      public uint TotalTerminatedProcesses;
    }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr CreateJobObject(IntPtr attributes, string name);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool SetInformationJobObject(IntPtr job, int infoClass, ref JOBOBJECT_EXTENDED_LIMIT_INFORMATION info, uint length);
    [DllImport("kernel32.dll", EntryPoint = "QueryInformationJobObject", SetLastError = true)]
    static extern bool QueryInformationJobObjectLimits(IntPtr job, int infoClass, ref JOBOBJECT_EXTENDED_LIMIT_INFORMATION info, uint length, out uint returned);
    [DllImport("kernel32.dll", EntryPoint = "QueryInformationJobObject", SetLastError = true)]
    static extern bool QueryInformationJobObjectAccounting(IntPtr job, int infoClass, ref JOBOBJECT_BASIC_ACCOUNTING_INFORMATION info, uint length, out uint returned);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern bool CreateProcess(string application, StringBuilder commandLine, IntPtr processAttributes, IntPtr threadAttributes, bool inheritHandles, uint flags, IntPtr environment, string currentDirectory, ref STARTUPINFO startup, out PROCESS_INFORMATION process);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool TerminateJobObject(IntPtr job, uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool CloseHandle(IntPtr handle);
  }
}
'@
}

function Start-FootprintJobProcess {
  param(
    [Parameter(Mandatory)][Diagnostics.ProcessStartInfo]$StartInfo,
    [ValidateRange(1, 33)][int]$ActiveProcessLimit = 33,
    [string[]]$ArgumentList = @()
  )
  if ($StartInfo.UseShellExecute -or -not [IO.Path]::IsPathRooted($StartInfo.FileName) -or
      -not [IO.Path]::IsPathRooted($StartInfo.WorkingDirectory) -or
      -not [string]::IsNullOrEmpty($StartInfo.Arguments)) {
    throw "Native Job launch requires one exact executable with no arguments."
  }
  $environment = @($StartInfo.EnvironmentVariables.Keys | Sort-Object | ForEach-Object {
    "$($_)=$($StartInfo.EnvironmentVariables[$_])"
  })
  $spawnStartedTick = [Diagnostics.Stopwatch]::GetTimestamp()
  $boundary = [SageFootprint.NativeJobBoundary]::Start(
    [IO.Path]::GetFullPath($StartInfo.FileName),
    [string[]]$ArgumentList,
    [IO.Path]::GetFullPath($StartInfo.WorkingDirectory),
    [string[]]$environment,
    [uint32]$ActiveProcessLimit
  )
  try {
    $process = [Diagnostics.Process]::GetProcessById($boundary.ProcessId)
  } catch {
    try { $boundary.Terminate() } finally { $boundary.Dispose() }
    throw
  }
  return [ordered]@{
    process = $process
    boundary = $boundary
    spawnStartedTick = $spawnStartedTick
  }
}

function Stop-FootprintJobProcess {
  param(
    [Parameter(Mandatory)]$Process,
    [Parameter(Mandatory)][SageFootprint.NativeJobBoundary]$Boundary,
    [ValidateRange(0, 5000)][int]$GraceMilliseconds = 1000,
    [ValidateRange(100, 5000)][int]$OrphanObservationMilliseconds = 5000
  )
  $frequency = [Diagnostics.Stopwatch]::Frequency
  $closeRequestedTick = [Diagnostics.Stopwatch]::GetTimestamp()
  $graceTicks = [long][Math]::Ceiling(($GraceMilliseconds * [double]$frequency) / 1000.0)
  $observationTicks = [long][Math]::Ceiling(($OrphanObservationMilliseconds * [double]$frequency) / 1000.0)
  $graceDeadline = $closeRequestedTick + $graceTicks
  $observationDeadline = $closeRequestedTick + $observationTicks
  try {
    try {
      $Process.Refresh()
      if (-not $Process.HasExited -and $Process.MainWindowHandle -ne [IntPtr]::Zero) {
        [void]$Process.CloseMainWindow()
      }
    } catch {
      # Job accounting remains authoritative when the root exits first.
    }
    $forced = $false
    $zeroDescendantsTick = $null
    while ([Diagnostics.Stopwatch]::GetTimestamp() -lt $observationDeadline) {
      $now = [Diagnostics.Stopwatch]::GetTimestamp()
      $active = [uint32]$Boundary.ActiveProcesses()
      if ($active -eq 0) {
        if ($null -eq $zeroDescendantsTick) { $zeroDescendantsTick = $now }
      } elseif ($null -ne $zeroDescendantsTick) {
        throw "Native Job became nonempty after reaching zero."
      } elseif ($now -ge $graceDeadline) {
        $Boundary.Terminate()
        $forced = $true
      }
      Start-Sleep -Milliseconds 10
    }
    $orphanCheckCompletedTick = [Diagnostics.Stopwatch]::GetTimestamp()
    $active = [uint32]$Boundary.ActiveProcesses()
    if ($active -ne 0 -or $null -eq $zeroDescendantsTick) {
      throw "Native Job did not remain empty through the orphan gate."
    }
    return [ordered]@{
      forced = $forced
      exited = $true
      treeBoundaryVerified = $true
      closeRequestedTick = $closeRequestedTick
      zeroDescendantsTick = [long]$zeroDescendantsTick
      orphanCheckCompletedTick = $orphanCheckCompletedTick
      orphanProcessesAfterClose = [int]$active
    }
  } catch {
    try { $Boundary.Terminate() } catch {}
    throw
  }
}

Export-ModuleMember -Function @(
  "Start-FootprintJobProcess",
  "Stop-FootprintJobProcess"
)
