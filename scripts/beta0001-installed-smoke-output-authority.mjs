import { spawn } from "node:child_process";
import { lstat, realpath, writeFile } from "node:fs/promises";
import path from "node:path";

const requestEnvironment = "SAGE_INSTALLED_SMOKE_OUTPUT_AUTHORITY";
const startupTimeoutMs = 10_000;
const outputMaximumBytes = 16 * 1024;
const authorities = new WeakSet();

const guardianSource = String.raw`
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$name = "SAGE_INSTALLED_SMOKE_OUTPUT_AUTHORITY"
$encoded = [Environment]::GetEnvironmentVariable($name, "Process")
[Environment]::SetEnvironmentVariable($name, $null, "Process")
$request = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($encoded)) | ConvertFrom-Json
$hostPath = (Get-Process -Id $PID -ErrorAction Stop).Path
$hostItem = Get-Item -LiteralPath $hostPath -Force -ErrorAction Stop
if ($hostItem.Length -ne [long]$request.powerShellBytes -or
    (Get-FileHash -LiteralPath $hostPath -Algorithm SHA256).Hash.ToLowerInvariant() -ne [string]$request.powerShellSha256) {
  throw "Output guardian PowerShell identity changed."
}
if (-not ("SageInstalledSmoke.DirectorySeal" -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
namespace SageInstalledSmoke {
  public sealed class DirectorySeal : IDisposable {
    const uint FILE_READ_ATTRIBUTES = 0x80, SHARE_READ = 1, SHARE_WRITE = 2;
    const uint OPEN_EXISTING = 3, BACKUP_SEMANTICS = 0x02000000, OPEN_REPARSE_POINT = 0x00200000;
    readonly List<SafeFileHandle> handles = new List<SafeFileHandle>();
    FileStream lockStream;
    string lockPath;
    public string Root { get; private set; }
    DirectorySeal(string root) { Root = root; }
    static SafeFileHandle OpenDirectory(string value) {
      var handle = CreateFile(value, FILE_READ_ATTRIBUTES, SHARE_READ | SHARE_WRITE,
        IntPtr.Zero, OPEN_EXISTING, BACKUP_SEMANTICS | OPEN_REPARSE_POINT, IntPtr.Zero);
      if (handle.IsInvalid) throw new IOException("Unable to seal directory: " + value,
        Marshal.GetLastWin32Error());
      return handle;
    }
    static void RequirePlainDirectory(string value) {
      var attributes = File.GetAttributes(value);
      if ((attributes & FileAttributes.Directory) == 0 ||
          (attributes & FileAttributes.ReparsePoint) != 0)
        throw new IOException("Output authority path is not a plain directory.");
    }
    public static DirectorySeal Acquire(string requested, bool create) {
      if (String.IsNullOrWhiteSpace(requested) || !Path.IsPathRooted(requested))
        throw new ArgumentException("Output authority path is invalid.");
      string root = Path.GetFullPath(requested).TrimEnd(Path.DirectorySeparatorChar);
      string volume = Path.GetPathRoot(root).TrimEnd(Path.DirectorySeparatorChar);
      if (String.Equals(root, volume, StringComparison.OrdinalIgnoreCase))
        throw new ArgumentException("Output authority root is ambiguous.");
      var seal = new DirectorySeal(root);
      try {
        var ancestors = new List<string>();
        var current = Directory.GetParent(root);
        while (current != null) { ancestors.Add(current.FullName); current = current.Parent; }
        ancestors.Reverse();
        foreach (string ancestor in ancestors) {
          RequirePlainDirectory(ancestor);
          var handle = OpenDirectory(ancestor);
          seal.handles.Add(handle);
          RequirePlainDirectory(ancestor);
        }
        if (create) {
          if (Directory.Exists(root) || File.Exists(root))
            throw new IOException("Output authority target already exists.");
          Directory.CreateDirectory(root);
        }
        RequirePlainDirectory(root);
        seal.handles.Add(OpenDirectory(root));
        RequirePlainDirectory(root);
        if (create && Directory.EnumerateFileSystemEntries(root).Any())
          throw new IOException("New output authority root is not empty.");
        seal.lockPath = Path.Combine(root, ".sage-output-authority.lock");
        seal.lockStream = new FileStream(seal.lockPath, FileMode.CreateNew,
          FileAccess.ReadWrite, FileShare.Read, 1, FileOptions.WriteThrough);
        return seal;
      } catch { seal.Dispose(); throw; }
    }
    public void Dispose() {
      if (lockStream != null) { lockStream.Dispose(); lockStream = null; }
      if (!String.IsNullOrEmpty(lockPath)) { try { File.Delete(lockPath); } catch {} }
      for (int i = handles.Count - 1; i >= 0; i--) handles[i].Dispose();
      handles.Clear();
      GC.SuppressFinalize(this);
    }
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern SafeFileHandle CreateFile(string name, uint access, uint share,
      IntPtr security, uint creation, uint flags, IntPtr template);
  }
}
'@
}
$seal = $null
try {
  $seal = [SageInstalledSmoke.DirectorySeal]::Acquire(
    [string]$request.outputDirectory,
    [bool]$request.create
  )
  [Console]::Out.WriteLine((@{schemaVersion=1; status="ready"; root=$seal.Root} | ConvertTo-Json -Compress))
  [Console]::Out.Flush()
  if ([Console]::In.ReadLine() -ne "release") { throw "Output authority release was not explicit." }
} finally {
  if ($null -ne $seal) { $seal.Dispose() }
}
`;

const guardianArguments = [
  "-NoLogo",
  "-NoProfile",
  "-NonInteractive",
  "-ExecutionPolicy",
  "Bypass",
  "-EncodedCommand",
  Buffer.from(guardianSource, "utf16le").toString("base64"),
];

async function ordinaryIdentity(root, label) {
  const item = await lstat(root, { bigint: true });
  const canonical = await realpath(root);
  if (
    !item.isDirectory() ||
    item.isSymbolicLink() ||
    canonical.toLowerCase() !== path.resolve(root).toLowerCase()
  ) {
    throw new Error(`${label}-invalid`);
  }
  return { dev: item.dev.toString(), ino: item.ino.toString() };
}

function stopChild(child) {
  if (child.exitCode === null) child.kill();
}

export async function acquireInstalledSmokeOutputAuthority({
  outputDirectory,
  powerShell,
  create = false,
}) {
  if (
    !path.isAbsolute(outputDirectory ?? "") ||
    outputDirectory.length > 4096 ||
    !path.isAbsolute(powerShell?.path ?? "") ||
    !/^[0-9a-f]{64}$/u.test(powerShell?.sha256 ?? "") ||
    !Number.isSafeInteger(powerShell?.bytes)
  ) {
    throw new Error("installed-smoke-output-authority-request-invalid");
  }
  const request = Buffer.from(
    JSON.stringify({
      outputDirectory: path.resolve(outputDirectory),
      create,
      powerShellSha256: powerShell.sha256,
      powerShellBytes: powerShell.bytes,
    }),
  ).toString("base64");
  const child = spawn(powerShell.path, guardianArguments, {
    cwd: path.dirname(path.resolve(outputDirectory)),
    env: { ...process.env, [requestEnvironment]: request },
    windowsHide: true,
    stdio: ["pipe", "pipe", "pipe"],
  });
  const authority = await new Promise((resolve, reject) => {
    let stdout = Buffer.alloc(0);
    let stderrBytes = 0;
    let settled = false;
    const fail = (error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      stopChild(child);
      reject(error);
    };
    const timer = setTimeout(
      () => fail(new Error("installed-smoke-output-authority-timeout")),
      startupTimeoutMs,
    );
    child.stdout.on("data", (chunk) => {
      if (settled) return;
      stdout = Buffer.concat([stdout, chunk]);
      if (stdout.byteLength > outputMaximumBytes) {
        fail(new Error("installed-smoke-output-authority-stdout-oversize"));
        return;
      }
      const newline = stdout.indexOf(0x0a);
      if (newline < 0) return;
      try {
        const value = JSON.parse(stdout.subarray(0, newline).toString("utf8"));
        if (
          value?.schemaVersion !== 1 ||
          value?.status !== "ready" ||
          value?.root?.toLowerCase() !==
            path.resolve(outputDirectory).toLowerCase()
        ) {
          throw new Error("installed-smoke-output-authority-proof-invalid");
        }
        settled = true;
        clearTimeout(timer);
        resolve({ root: path.resolve(outputDirectory), child });
      } catch (error) {
        fail(error);
      }
    });
    child.stderr.on("data", (chunk) => {
      stderrBytes += chunk.byteLength;
      if (stderrBytes > outputMaximumBytes) {
        fail(new Error("installed-smoke-output-authority-stderr-oversize"));
      }
    });
    child.on("error", fail);
    child.on("close", (code) => {
      if (!settled)
        fail(new Error(`installed-smoke-output-authority-exit-${code}`));
    });
  });
  try {
    authority.identity = await ordinaryIdentity(
      authority.root,
      "installed-smoke-output-authority",
    );
  } catch (error) {
    authority.child.stdin.end("release\n");
    stopChild(authority.child);
    throw error;
  }
  authorities.add(authority);
  return authority;
}

export async function assertInstalledSmokeOutputAuthority(authority, label) {
  if (!authorities.has(authority) || authority.child.exitCode !== null) {
    throw new Error(`${label}-lost`);
  }
  const observed = await ordinaryIdentity(authority.root, label);
  if (
    observed.dev !== authority.identity.dev ||
    observed.ino !== authority.identity.ino
  ) {
    throw new Error(`${label}-changed`);
  }
  return authority.root;
}

export async function writeInstalledSmokeOutputFile(
  authority,
  relativePath,
  bytes,
  options,
) {
  if (relativePath !== "orchestrator-failure.json") {
    throw new Error("installed-smoke-output-write-relative-path-invalid");
  }
  const root = await assertInstalledSmokeOutputAuthority(
    authority,
    "installed-smoke-output-write-authority",
  );
  await writeFile(path.join(root, relativePath), bytes, options);
}

export async function releaseInstalledSmokeOutputAuthority(authority) {
  if (!authorities.has(authority)) return;
  authorities.delete(authority);
  if (authority.child.exitCode !== null) return;
  // The guardian holds no-delete handles on the output's ancestors until its
  // process is gone, so wait for it to close even when it has to be stopped.
  await new Promise((resolve) => {
    const closed = setTimeout(resolve, 10_000);
    const stop = setTimeout(() => stopChild(authority.child), 5000);
    authority.child.once("close", () => {
      clearTimeout(stop);
      clearTimeout(closed);
      resolve();
    });
    authority.child.stdin.end("release\n");
  });
}

export const INSTALLED_SMOKE_OUTPUT_GUARDIAN_ARGUMENTS = guardianArguments;
