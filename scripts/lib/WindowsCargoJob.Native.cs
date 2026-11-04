using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;

namespace SageValidation {
  public sealed class ProtocolWriter {
    readonly Stream output;
    readonly string requestNonce;
    readonly object gate = new object();
    long sequence;

    public ProtocolWriter(Stream output, string requestNonce) {
      this.output = output;
      this.requestNonce = requestNonce;
    }

    public long Sequence {
      get { lock (gate) { return sequence; } }
    }

    public void Data(string stream, byte[] bytes, int count) {
      string encoded = Convert.ToBase64String(bytes, 0, count);
      Frame("data", "\"stream\":\"" + stream +
        "\",\"data\":\"" + encoded + "\"");
    }

    public void Truncated(string stream, string kind) {
      Frame("truncated", "\"stream\":\"" + stream +
        "\",\"kind\":\"" + kind + "\"");
    }

    void Frame(string type, string fields) {
      lock (gate) {
        string value = "{\"type\":\"" + type + "\",\"schemaVersion\":1," +
          "\"requestNonce\":\"" + requestNonce + "\",\"sequence\":" +
          sequence + "," + fields + "}\n";
        sequence++;
        byte[] bytes = Encoding.UTF8.GetBytes(value);
        output.Write(bytes, 0, bytes.Length);
        output.Flush();
      }
    }
  }

  public sealed class FramedCaptureStream : Stream {
    const int MaximumFrameBytes = 4096;
    readonly ProtocolWriter protocol;
    readonly string stream;
    readonly int maximumLineBytes;
    readonly int maximumOutputBytes;
    int lineBytes;
    int outputBytes;
    public volatile string Overflow;

    public FramedCaptureStream(
      ProtocolWriter protocol,
      string stream,
      int maximumLineBytes,
      int maximumOutputBytes
    ) {
      this.protocol = protocol;
      this.stream = stream;
      this.maximumLineBytes = maximumLineBytes;
      this.maximumOutputBytes = maximumOutputBytes;
    }

    public override void Write(byte[] buffer, int offset, int count) {
      int end = offset + count;
      var frame = new byte[MaximumFrameBytes];
      int frameBytes = 0;
      for (int index = offset; index < end && Overflow == null; index++) {
        byte value = buffer[index];
        string overflow = null;
        if (outputBytes >= maximumOutputBytes) overflow = "total";
        else if (value != 10 && lineBytes >= maximumLineBytes) overflow = "line";
        if (overflow != null) {
          FlushFrame(frame, ref frameBytes);
          Overflow = overflow;
          protocol.Truncated(stream, overflow);
          break;
        }
        frame[frameBytes++] = value;
        outputBytes++;
        lineBytes = value == 10 ? 0 : lineBytes + 1;
        if (frameBytes == frame.Length) FlushFrame(frame, ref frameBytes);
      }
      FlushFrame(frame, ref frameBytes);
    }

    void FlushFrame(byte[] frame, ref int count) {
      if (count == 0) return;
      protocol.Data(stream, frame, count);
      count = 0;
    }

    public override void Flush() {}
    public override bool CanRead { get { return false; } }
    public override bool CanSeek { get { return false; } }
    public override bool CanWrite { get { return true; } }
    public override long Length { get { return outputBytes; } }
    public override long Position {
      get { return outputBytes; }
      set { throw new NotSupportedException(); }
    }
    public override int Read(byte[] b, int o, int n) { throw new NotSupportedException(); }
    public override long Seek(long o, SeekOrigin x) { throw new NotSupportedException(); }
    public override void SetLength(long value) { throw new NotSupportedException(); }
  }

  public sealed class BoundedDiscardStream : Stream {
    readonly int maximumBytes;
    int observedBytes;
    public bool Overflow { get; private set; }

    public BoundedDiscardStream(int maximumBytes) {
      this.maximumBytes = maximumBytes;
    }
    public override void Write(byte[] buffer, int offset, int count) {
      observedBytes = Math.Min(maximumBytes + 1, observedBytes + count);
      if (observedBytes > maximumBytes) Overflow = true;
    }
    public override void Flush() {}
    public override bool CanRead { get { return false; } }
    public override bool CanSeek { get { return false; } }
    public override bool CanWrite { get { return true; } }
    public override long Length { get { return observedBytes; } }
    public override long Position {
      get { return observedBytes; }
      set { throw new NotSupportedException(); }
    }
    public override int Read(byte[] b, int o, int n) { throw new NotSupportedException(); }
    public override long Seek(long o, SeekOrigin x) { throw new NotSupportedException(); }
    public override void SetLength(long value) { throw new NotSupportedException(); }
  }

  public sealed class CargoJob : IDisposable {
    const uint CREATE_SUSPENDED = 0x00000004;
    const uint CREATE_UNICODE_ENVIRONMENT = 0x00000400;
    const uint EXTENDED_STARTUPINFO_PRESENT = 0x00080000;
    const uint PROC_THREAD_ATTRIBUTE_HANDLE_LIST = 0x00020002;
    const uint JOB_OBJECT_LIMIT_ACTIVE_PROCESS = 0x00000008;
    const uint JOB_OBJECT_LIMIT_BREAKAWAY_OK = 0x00000800;
    const uint JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK = 0x00001000;
    const uint JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000;
    const uint PROCESS_TERMINATE = 0x0001;
    const uint PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;
    const uint SYNCHRONIZE = 0x00100000;
    const uint WAIT_OBJECT_0 = 0;
    const uint WAIT_TIMEOUT = 0x00000102;
    const int JobObjectBasicProcessIdList = 3;
    const int MaximumJobProcessIds = 256;
    const int JobObjectBasicAccountingInformation = 1;
    const int JobObjectExtendedLimitInformation = 9;
    const uint VCTIP_TERMINATION_EXIT_CODE = 0x53475654;
    const uint VCTIP_TERMINATION_WAIT_MS = 2000;
    const string VCTIP_PATH = "C:\\Program Files (x86)\\Microsoft Visual Studio\\2022\\BuildTools\\VC\\Tools\\MSVC\\14.44.35207\\bin\\HostX64\\x64\\VCTIP.EXE";
    const string VCTIP_FILENAME = "VCTIP.EXE";
    const long VCTIP_SIZE = 335328;
    const string VCTIP_SHA256 = "e175c981ca866689bf8f8d9bd5ba1e71e2eb1c9d389de555744b0cb439250996";
    const string VCTIP_VERSION = "14.44.35228.0";
    IntPtr job;
    IntPtr process;
    uint rootProcessId;
#if SAGE_VCTIP_JOB_TESTS
    // Test-only state is per Job instance, never global, and is absent from
    // production compilation. It permits regressions to exercise the retained
    // handle/race path without adding a request-controlled production seam.
    uint vctipTestMember;
    string vctipTestNaturalExitTriggerPath;
    uint vctipTestNaturalExitHooks;
    int vctipTestNaturalExitCode = -1;
#if SAGE_VCTIP_JOB_TEST_WAIT_FAILED
    uint vctipTestUnconfirmedObservation = 1;
#elif SAGE_VCTIP_JOB_TEST_WAIT_TIMEOUT
    uint vctipTestUnconfirmedObservation = 2;
#elif SAGE_VCTIP_JOB_TEST_EXIT_UNCONFIRMED
    uint vctipTestUnconfirmedObservation = 3;
#else
    uint vctipTestUnconfirmedObservation;
#endif
    uint vctipTestTerminationAttempts;
#endif

    CargoJob(IntPtr job, IntPtr process, uint rootProcessId) {
      this.job = job;
      this.process = process;
      this.rootProcessId = rootProcessId;
    }

    public static CargoJob Start(
      string executable,
      string[] arguments,
      string workingDirectory,
      string[] environmentEntries,
      uint activeProcessLimit,
      out FileStream stdout,
      out FileStream stderr
    ) {
      IntPtr job = IntPtr.Zero, process = IntPtr.Zero, thread = IntPtr.Zero;
      IntPtr environment = IntPtr.Zero, attributes = IntPtr.Zero, handles = IntPtr.Zero;
      IntPtr outRead = IntPtr.Zero, outWrite = IntPtr.Zero;
      IntPtr errRead = IntPtr.Zero, errWrite = IntPtr.Zero, nullRead = IntPtr.Zero;
      bool assigned = false;
      try {
        job = CreateJobObject(IntPtr.Zero, null);
        if (job == IntPtr.Zero) NativeFailure("CreateJobObject");
        Configure(job, activeProcessLimit);
        var security = new SECURITY_ATTRIBUTES {
          nLength = Marshal.SizeOf(typeof(SECURITY_ATTRIBUTES)),
          bInheritHandle = true
        };
        if (!CreatePipe(out outRead, out outWrite, ref security, 0) ||
            !CreatePipe(out errRead, out errWrite, ref security, 0)) NativeFailure("CreatePipe");
        if (!SetHandleInformation(outRead, 1, 0) ||
            !SetHandleInformation(errRead, 1, 0)) NativeFailure("SetHandleInformation");
        nullRead = CreateFile("NUL", 0x80000000, 3, ref security, 3, 0, IntPtr.Zero);
        if (nullRead == new IntPtr(-1)) NativeFailure("CreateFileNUL");
        IntPtr attributeBytes = IntPtr.Zero;
        InitializeProcThreadAttributeList(IntPtr.Zero, 1, 0, ref attributeBytes);
        attributes = Marshal.AllocHGlobal(attributeBytes);
        if (!InitializeProcThreadAttributeList(attributes, 1, 0, ref attributeBytes))
          NativeFailure("InitializeProcThreadAttributeList");
        handles = Marshal.AllocHGlobal(IntPtr.Size * 3);
        Marshal.WriteIntPtr(handles, 0, nullRead);
        Marshal.WriteIntPtr(handles, IntPtr.Size, outWrite);
        Marshal.WriteIntPtr(handles, IntPtr.Size * 2, errWrite);
        if (!UpdateProcThreadAttribute(
          attributes, 0, new IntPtr(PROC_THREAD_ATTRIBUTE_HANDLE_LIST),
          handles, new IntPtr(IntPtr.Size * 3), IntPtr.Zero, IntPtr.Zero
        )) NativeFailure("UpdateProcThreadAttribute");
        var startup = new STARTUPINFOEX();
        startup.StartupInfo.cb = Marshal.SizeOf(typeof(STARTUPINFOEX));
        startup.StartupInfo.dwFlags = 0x00000100;
        startup.StartupInfo.hStdInput = nullRead;
        startup.StartupInfo.hStdOutput = outWrite;
        startup.StartupInfo.hStdError = errWrite;
        startup.lpAttributeList = attributes;
        environment = Marshal.StringToHGlobalUni(EnvironmentBlock(environmentEntries));
        var commandLine = new StringBuilder(BuildCommandLine(executable, arguments));
        var processInfo = new PROCESS_INFORMATION();
        if (!CreateProcess(
          executable, commandLine, IntPtr.Zero, IntPtr.Zero, true,
          CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
          environment, workingDirectory, ref startup, out processInfo
        )) NativeFailure("CreateProcessW");
        process = processInfo.hProcess;
        thread = processInfo.hThread;
        if (!AssignProcessToJobObject(job, process)) NativeFailure("AssignProcessToJobObject");
        assigned = true;
        if (ResumeThread(thread) == UInt32.MaxValue) NativeFailure("ResumeThread");
        CloseHandle(outWrite); outWrite = IntPtr.Zero;
        CloseHandle(errWrite); errWrite = IntPtr.Zero;
        CloseHandle(nullRead); nullRead = IntPtr.Zero;
        stdout = OwnedPipe(outRead); outRead = IntPtr.Zero;
        stderr = OwnedPipe(errRead); errRead = IntPtr.Zero;
        return new CargoJob(job, process, processInfo.dwProcessId);
      } catch {
        if (process != IntPtr.Zero) {
          if (assigned) TerminateJobObject(job, 1); else TerminateProcess(process, 1);
          WaitForSingleObject(process, 5000);
        }
        if (job != IntPtr.Zero) CloseHandle(job);
        throw;
      } finally {
        CloseIfPresent(thread); CloseIfPresent(outRead); CloseIfPresent(outWrite);
        CloseIfPresent(errRead); CloseIfPresent(errWrite); CloseIfPresent(nullRead);
        if (environment != IntPtr.Zero) Marshal.FreeHGlobal(environment);
        if (attributes != IntPtr.Zero) {
          DeleteProcThreadAttributeList(attributes);
          Marshal.FreeHGlobal(attributes);
        }
        if (handles != IntPtr.Zero) Marshal.FreeHGlobal(handles);
      }
    }

    static FileStream OwnedPipe(IntPtr handle) {
      return new FileStream(
        new Microsoft.Win32.SafeHandles.SafeFileHandle(handle, true),
        FileAccess.Read, 4096, false
      );
    }

    static string EnvironmentBlock(string[] entries) {
      var names = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
      foreach (string entry in entries) {
        int equals = entry == null ? -1 : entry.IndexOf('=');
        if (equals < 1 || entry.IndexOf('\0') >= 0 || !names.Add(entry.Substring(0, equals)))
          throw new ArgumentException("environment-invalid");
      }
      return String.Join("\0", entries.OrderBy(v => v, StringComparer.OrdinalIgnoreCase)) + "\0\0";
    }

    static string BuildCommandLine(string executable, string[] arguments) {
      var command = new StringBuilder(QuoteArgument(executable));
      foreach (string argument in arguments) command.Append(' ').Append(QuoteArgument(argument));
      if (command.Length >= 32767) throw new ArgumentException("command-line-oversize");
      return command.ToString();
    }

    static string QuoteArgument(string argument) {
      if (argument == null || argument.IndexOf('\0') >= 0) throw new ArgumentException("argument-invalid");
      if (argument.Length > 0 && argument.IndexOfAny(new [] {' ', '\t', '\r', '\n', '\v', '\f', '"'}) < 0) return argument;
      var quoted = new StringBuilder("\"");
      int slashes = 0;
      foreach (char value in argument) {
        if (value == '\\') { slashes++; continue; }
        quoted.Append('\\', value == '"' ? slashes * 2 + 1 : slashes);
        quoted.Append(value); slashes = 0;
      }
      return quoted.Append('\\', slashes * 2).Append('"').ToString();
    }

    static void Configure(IntPtr job, uint activeProcessLimit) {
      var limits = new JOBOBJECT_EXTENDED_LIMIT_INFORMATION();
      limits.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
      limits.BasicLimitInformation.ActiveProcessLimit = activeProcessLimit;
      if (!SetInformationJobObject(
        job, JobObjectExtendedLimitInformation, ref limits, (uint)Marshal.SizeOf(limits)
      )) NativeFailure("SetInformationJobObject");
      var actual = new JOBOBJECT_EXTENDED_LIMIT_INFORMATION();
      uint returned;
      if (!QueryInformationJobObjectLimits(
        job, JobObjectExtendedLimitInformation, ref actual,
        (uint)Marshal.SizeOf(actual), out returned
      )) NativeFailure("QueryInformationJobObject");
      uint flags = actual.BasicLimitInformation.LimitFlags;
      uint forbidden = JOB_OBJECT_LIMIT_BREAKAWAY_OK | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK;
      if ((flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE) == 0 ||
          (flags & JOB_OBJECT_LIMIT_ACTIVE_PROCESS) == 0 ||
          (flags & forbidden) != 0 ||
          actual.BasicLimitInformation.ActiveProcessLimit != activeProcessLimit)
        throw new InvalidOperationException("job-limits-invalid");
    }

    public uint ActiveProcesses() {
      var accounting = new JOBOBJECT_BASIC_ACCOUNTING_INFORMATION();
      uint returned;
      if (!QueryInformationJobObjectAccounting(
        job, JobObjectBasicAccountingInformation, ref accounting,
        (uint)Marshal.SizeOf(accounting), out returned
      )) NativeFailure("QueryInformationJobObject");
      return accounting.ActiveProcesses;
    }
    // Job-wide CPU time and process counts, for diagnosing a command that
    // outlives its deadline: CPU time near the wall time means its members
    // were computing, and little CPU time means they were waiting.
    public string AccountingSummary() {
      var accounting = new JOBOBJECT_BASIC_ACCOUNTING_INFORMATION();
      uint returned;
      if (!QueryInformationJobObjectAccounting(
        job, JobObjectBasicAccountingInformation, ref accounting,
        (uint)Marshal.SizeOf(accounting), out returned
      )) NativeFailure("QueryInformationJobObject");
      return "userMs=" + (accounting.a / 10000) +
        " kernelMs=" + (accounting.b / 10000) +
        " totalProcesses=" + accounting.f +
        " activeProcesses=" + accounting.ActiveProcesses +
        " pageFaults=" + accounting.e;
    }
    // File names of the Job's live members, for diagnosing a command that
    // outlives its deadline. A member whose image cannot be read is "?".
    public string[] MemberImageNames() {
      uint[] processIds;
      if (!TryJobProcessIds(out processIds)) return new string[0];
      var names = new string[processIds.Length];
      for (int index = 0; index < processIds.Length; index++) {
        names[index] = "?";
        IntPtr member = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, processIds[index]);
        if (member == IntPtr.Zero) continue;
        try {
          string imagePath;
          if (TryImagePath(member, out imagePath)) names[index] = Path.GetFileName(imagePath);
        } finally {
          CloseIfPresent(member);
        }
      }
      Array.Sort(names, StringComparer.OrdinalIgnoreCase);
      return names;
    }
    public uint RootProcessId { get { return rootProcessId; } }
    public bool RootRunning {
      get {
        uint wait = WaitForSingleObject(process, 0);
        if (wait == WAIT_TIMEOUT) return true;
        if (wait == WAIT_OBJECT_0) return false;
        NativeFailure("WaitForSingleObject");
        return false;
      }
    }
    public int ExitCode() {
      uint code;
      if (!GetExitCodeProcess(process, out code)) NativeFailure("GetExitCodeProcess");
      return unchecked((int)code);
    }
    public void Terminate() {
      if (!TerminateJobObject(job, 1)) NativeFailure("TerminateJobObject");
    }
#if SAGE_VCTIP_JOB_TESTS
    internal void EnableVctipTeardownTestAdapter(
      uint member, uint unconfirmedObservation, string naturalExitTriggerPath
    ) {
      if (member == 0 || unconfirmedObservation > 3 ||
          (naturalExitTriggerPath != null &&
           (!Path.IsPathRooted(naturalExitTriggerPath) || naturalExitTriggerPath.Length > 32767)))
        throw new ArgumentException("vctip-test-adapter-invalid");
      vctipTestMember = member;
      vctipTestUnconfirmedObservation = unconfirmedObservation;
      vctipTestNaturalExitTriggerPath = naturalExitTriggerPath;
    }
    internal uint VctipTeardownTestTerminationAttempts {
      get { return vctipTestTerminationAttempts; }
    }
    internal uint VctipTeardownTestNaturalExitHooks {
      get { return vctipTestNaturalExitHooks; }
    }
    internal int VctipTeardownTestNaturalExitCode {
      get { return vctipTestNaturalExitCode; }
    }
    internal int VctipTeardownTestJobProcessCount {
      get {
        uint[] processIds;
        return TryJobProcessIds(out processIds) ? processIds.Length : -1;
      }
    }
#endif
    // This is deliberately compiled into the helper rather than supplied by a
    // request, environment variable, or caller allowlist. Any MSVC update must
    // obtain a separately reviewed identity before it can match.
    public object TryTeardownVerifiedVctip() {
      uint candidate;
      if (!RootExited() || !TrySingletonJobMember(out candidate)) return null;
      IntPtr member = OpenProcess(
        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | SYNCHRONIZE,
        false, candidate
      );
      if (member == IntPtr.Zero) return null;
      try {
        if (!MatchesVerifiedVctip(member, candidate)) return null;
        // Revalidate every mutable fact while retaining this one private handle.
        if (!RootExited() || !TrySingletonJobMember(out candidate) ||
            !MatchesVerifiedVctip(member, candidate)) return null;
        // Count only an actual targeted TerminateProcess call in test builds.
#if SAGE_VCTIP_JOB_TESTS
        vctipTestTerminationAttempts++;
#endif
        if (!TerminateProcess(member, VCTIP_TERMINATION_EXIT_CODE))
          throw new InvalidOperationException("vctip-termination-request-failed");
#if SAGE_VCTIP_JOB_TESTS
        if (vctipTestUnconfirmedObservation == 1)
          return UnconfirmedVctipTeardown("wait-failed", candidate);
        if (vctipTestUnconfirmedObservation == 2)
          return UnconfirmedVctipTeardown("wait-timeout", candidate);
#endif
        uint waited = WaitForSingleObject(member, VCTIP_TERMINATION_WAIT_MS);
        if (waited != WAIT_OBJECT_0) return UnconfirmedVctipTeardown(
          waited == WAIT_TIMEOUT ? "wait-timeout" : "wait-failed", candidate
        );
#if SAGE_VCTIP_JOB_TESTS
        if (vctipTestUnconfirmedObservation == 3)
          return UnconfirmedVctipTeardown("exit-code-unconfirmed", candidate);
#endif
        uint observed;
        if (!GetExitCodeProcess(member, out observed) || observed != VCTIP_TERMINATION_EXIT_CODE)
          return UnconfirmedVctipTeardown("exit-code-unconfirmed", candidate);
        return new VctipTeardownReceipt {
          kind = "verified-msvc-vctip",
          imagePath = VCTIP_PATH,
          originalFilename = VCTIP_FILENAME,
          imageSizeBytes = VCTIP_SIZE,
          sha256 = VCTIP_SHA256,
          fileVersion = VCTIP_VERSION,
          productVersion = VCTIP_VERSION,
          singletonPid = candidate,
          rootExited = true,
          jobMembershipVerified = true,
          terminationExitCodeRequested = VCTIP_TERMINATION_EXIT_CODE,
          terminationExitCodeObserved = observed
        };
      } finally {
        CloseIfPresent(member);
      }
    }
    VctipTeardownUnconfirmed UnconfirmedVctipTeardown(string observation, uint candidate) {
      return new VctipTeardownUnconfirmed {
        kind = "msvc-vctip-termination-unconfirmed",
        imagePath = VCTIP_PATH,
        originalFilename = VCTIP_FILENAME,
        imageSizeBytes = VCTIP_SIZE,
        sha256 = VCTIP_SHA256,
        fileVersion = VCTIP_VERSION,
        productVersion = VCTIP_VERSION,
        singletonPid = candidate,
        rootExited = true,
        jobMembershipVerified = true,
        terminationExitCodeRequested = VCTIP_TERMINATION_EXIT_CODE,
        terminationObservation = observation
      };
    }
    bool RootExited() {
      return WaitForSingleObject(process, 0) == WAIT_OBJECT_0;
    }
    bool TrySingletonJobMember(out uint member) {
      member = 0;
      uint[] processIds;
      if (!TryJobProcessIds(out processIds) || processIds.Length != 1 ||
          processIds[0] == 0 || processIds[0] == rootProcessId) return false;
      member = processIds[0];
      return true;
    }
    bool TryJobProcessIds(out uint[] processIds) {
      processIds = null;
      uint returned;
      int capacity = 8 + IntPtr.Size * MaximumJobProcessIds;
      IntPtr buffer = Marshal.AllocHGlobal(capacity);
      try {
        if (!QueryInformationJobObjectProcessList(job, JobObjectBasicProcessIdList,
          buffer, (uint)capacity, out returned)) return false;
        uint assigned = unchecked((uint)Marshal.ReadInt32(buffer, 0));
        uint listed = unchecked((uint)Marshal.ReadInt32(buffer, 4));
        long bytes = 8L + (long)listed * IntPtr.Size;
        if (returned < 8 || returned > capacity || assigned != listed ||
            listed > MaximumJobProcessIds || bytes != returned) return false;
        var values = new uint[(int)listed];
        for (int index = 0; index < values.Length; index++) {
          long raw = IntPtr.Size == 8
            ? Marshal.ReadInt64(buffer, 8 + index * IntPtr.Size)
            : unchecked((uint)Marshal.ReadInt32(buffer, 8 + index * IntPtr.Size));
          if (raw < 1 || raw > UInt32.MaxValue || values.Take(index).Contains((uint)raw))
            return false;
          values[index] = (uint)raw;
        }
        processIds = values;
        return true;
      } finally {
        Marshal.FreeHGlobal(buffer);
      }
    }
    bool MatchesVerifiedVctip(IntPtr member, uint candidate) {
      uint processId = GetProcessId(member);
      bool inJob;
      if (processId != candidate || !IsProcessInJob(member, job, out inJob) || !inJob)
        return false;
#if SAGE_VCTIP_JOB_TESTS
      if (vctipTestMember == 0) vctipTestMember = candidate;
      if (candidate == vctipTestMember) {
        if (vctipTestNaturalExitTriggerPath != null) {
          if (vctipTestNaturalExitHooks != 0)
            throw new InvalidOperationException("vctip-test-natural-exit-repeated");
          File.WriteAllText(vctipTestNaturalExitTriggerPath, "exit");
          vctipTestNaturalExitHooks++;
          if (WaitForSingleObject(member, 1000) != WAIT_OBJECT_0)
            throw new InvalidOperationException("vctip-test-natural-exit-unobserved");
          uint naturalExitCode;
          if (!GetExitCodeProcess(member, out naturalExitCode) || naturalExitCode != 0)
            throw new InvalidOperationException("vctip-test-natural-exit-invalid");
          vctipTestNaturalExitCode = 0;
          vctipTestNaturalExitTriggerPath = null;
        }
        return true;
      }
#endif
      string imagePath;
      if (!TryImagePath(member, out imagePath) ||
          !String.Equals(imagePath, VCTIP_PATH, StringComparison.OrdinalIgnoreCase) ||
          !String.Equals(Path.GetFileName(imagePath), VCTIP_FILENAME, StringComparison.OrdinalIgnoreCase))
        return false;
      try {
        var info = new FileInfo(imagePath);
        var version = FileVersionInfo.GetVersionInfo(imagePath);
        if (info.Length != VCTIP_SIZE ||
            !String.Equals(version.OriginalFilename, VCTIP_FILENAME, StringComparison.Ordinal) ||
            !String.Equals(version.FileVersion, VCTIP_VERSION, StringComparison.Ordinal) ||
            !String.Equals(version.ProductVersion, VCTIP_VERSION, StringComparison.Ordinal)) return false;
        using (var file = File.OpenRead(imagePath))
        using (var sha256 = SHA256.Create()) {
          string hash = BitConverter.ToString(sha256.ComputeHash(file)).Replace("-", "").ToLowerInvariant();
          return String.Equals(hash, VCTIP_SHA256, StringComparison.Ordinal);
        }
      } catch {
        return false;
      }
    }
    static bool TryImagePath(IntPtr handle, out string imagePath) {
      var value = new StringBuilder(32768);
      uint length = (uint)value.Capacity;
      if (!QueryFullProcessImageName(handle, 0, value, ref length) || length == 0) {
        imagePath = null;
        return false;
      }
      imagePath = value.ToString();
      return true;
    }
    public void Dispose() {
      IntPtr processHandle = process; process = IntPtr.Zero; CloseIfPresent(processHandle);
      IntPtr jobHandle = job; job = IntPtr.Zero; CloseIfPresent(jobHandle);
      GC.SuppressFinalize(this);
    }
    ~CargoJob() { Dispose(); }
    static void CloseIfPresent(IntPtr handle) {
      if (handle != IntPtr.Zero && handle != new IntPtr(-1)) CloseHandle(handle);
    }
    static void NativeFailure(string operation) {
      throw new Win32Exception(Marshal.GetLastWin32Error(), operation + " failed");
    }

    [StructLayout(LayoutKind.Sequential)] struct SECURITY_ATTRIBUTES { public int nLength; public IntPtr lpSecurityDescriptor; [MarshalAs(UnmanagedType.Bool)] public bool bInheritHandle; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct STARTUPINFO { public int cb; public string lpReserved, lpDesktop, lpTitle; public uint dwX,dwY,dwXSize,dwYSize,dwXCountChars,dwYCountChars,dwFillAttribute,dwFlags; public ushort wShowWindow,cbReserved2; public IntPtr lpReserved2,hStdInput,hStdOutput,hStdError; }
    [StructLayout(LayoutKind.Sequential)] struct STARTUPINFOEX { public STARTUPINFO StartupInfo; public IntPtr lpAttributeList; }
    [StructLayout(LayoutKind.Sequential)] struct PROCESS_INFORMATION { public IntPtr hProcess,hThread; public uint dwProcessId,dwThreadId; }
    [StructLayout(LayoutKind.Sequential)] struct BASIC_LIMIT { public long a,b; public uint LimitFlags; public UIntPtr c,d; public uint ActiveProcessLimit; public UIntPtr e; public uint f,g; }
    [StructLayout(LayoutKind.Sequential)] struct IO_COUNTERS { public ulong a,b,c,d,e,f; }
    [StructLayout(LayoutKind.Sequential)] struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION { public BASIC_LIMIT BasicLimitInformation; public IO_COUNTERS IoInfo; public UIntPtr a,b,c,d; }
    [StructLayout(LayoutKind.Sequential)] struct JOBOBJECT_BASIC_ACCOUNTING_INFORMATION { public long a,b,c,d; public uint e,f,ActiveProcesses,h; }
    public sealed class VctipTeardownReceipt {
      public string kind { get; set; }
      public string imagePath { get; set; }
      public string originalFilename { get; set; }
      public long imageSizeBytes { get; set; }
      public string sha256 { get; set; }
      public string fileVersion { get; set; }
      public string productVersion { get; set; }
      public uint singletonPid { get; set; }
      public bool rootExited { get; set; }
      public bool jobMembershipVerified { get; set; }
      public uint terminationExitCodeRequested { get; set; }
      public uint terminationExitCodeObserved { get; set; }
    }
    public sealed class VctipTeardownUnconfirmed {
      public string kind { get; set; }
      public string imagePath { get; set; }
      public string originalFilename { get; set; }
      public long imageSizeBytes { get; set; }
      public string sha256 { get; set; }
      public string fileVersion { get; set; }
      public string productVersion { get; set; }
      public uint singletonPid { get; set; }
      public bool rootExited { get; set; }
      public bool jobMembershipVerified { get; set; }
      public uint terminationExitCodeRequested { get; set; }
      public string terminationObservation { get; set; }
    }
    [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr CreateJobObject(IntPtr a,string b);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetInformationJobObject(IntPtr a,int b,ref JOBOBJECT_EXTENDED_LIMIT_INFORMATION c,uint d);
    [DllImport("kernel32.dll", EntryPoint="QueryInformationJobObject", SetLastError=true)] static extern bool QueryInformationJobObjectLimits(IntPtr a,int b,ref JOBOBJECT_EXTENDED_LIMIT_INFORMATION c,uint d,out uint e);
    [DllImport("kernel32.dll", EntryPoint="QueryInformationJobObject", SetLastError=true)] static extern bool QueryInformationJobObjectAccounting(IntPtr a,int b,ref JOBOBJECT_BASIC_ACCOUNTING_INFORMATION c,uint d,out uint e);
    [DllImport("kernel32.dll", EntryPoint="QueryInformationJobObject", SetLastError=true)] static extern bool QueryInformationJobObjectProcessList(IntPtr a,int b,IntPtr c,uint d,out uint e);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool AssignProcessToJobObject(IntPtr a,IntPtr b);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool TerminateJobObject(IntPtr a,uint b);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool CreatePipe(out IntPtr a,out IntPtr b,ref SECURITY_ATTRIBUTES c,uint d);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetHandleInformation(IntPtr a,uint b,uint c);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool InitializeProcThreadAttributeList(IntPtr a,int b,int c,ref IntPtr d);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool UpdateProcThreadAttribute(IntPtr a,uint b,IntPtr c,IntPtr d,IntPtr e,IntPtr f,IntPtr g);
    [DllImport("kernel32.dll")] static extern void DeleteProcThreadAttributeList(IntPtr a);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CreateFile(string a,uint b,uint c,ref SECURITY_ATTRIBUTES d,uint e,uint f,IntPtr g);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool CreateProcess(string a,StringBuilder b,IntPtr c,IntPtr d,bool e,uint f,IntPtr g,string h,ref STARTUPINFOEX i,out PROCESS_INFORMATION j);
    [DllImport("kernel32.dll", SetLastError=true)] static extern uint ResumeThread(IntPtr a);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool TerminateProcess(IntPtr a,uint b);
    [DllImport("kernel32.dll", SetLastError=true)] static extern uint WaitForSingleObject(IntPtr a,uint b);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr a,out uint b);
    [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint a,bool b,uint c);
    [DllImport("kernel32.dll", SetLastError=true)] static extern uint GetProcessId(IntPtr a);
    [DllImport("kernel32.dll", SetLastError=true)] [return: MarshalAs(UnmanagedType.Bool)] static extern bool IsProcessInJob(IntPtr a,IntPtr b,out bool c);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] [return: MarshalAs(UnmanagedType.Bool)] static extern bool QueryFullProcessImageName(IntPtr a,uint b,StringBuilder c,ref uint d);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool CloseHandle(IntPtr a);
  }
}
