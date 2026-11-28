// The launcher's children and everything they start (cargo, rustc, the debug
// app and its harnesses) stop together. Each root is remembered for the
// launcher's lifetime, even after it exits, so its descendants stay reachable.
import { spawn, spawnSync } from "node:child_process";

const roots = [];

export function launch(args, { cwd, env }) {
  const child = spawn(process.execPath, args, {
    cwd,
    env,
    stdio: ["ignore", "inherit", "inherit"],
    // POSIX: its own process group, which outlives an exited leader.
    detached: process.platform !== "win32",
  });
  if (child.pid !== undefined)
    roots.push({ pid: child.pid, started: Date.now() });
  return child;
}

// Windows keeps a process's parent id after the parent exits, so the tree is
// rebuilt from a process snapshot. A process counts only if it started after
// its parent, which rules out a reused process id.
const WINDOWS_SWEEP = `
$ErrorActionPreference = 'SilentlyContinue'
$all = @(Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, CreationDate)
$doomed = New-Object 'System.Collections.Generic.HashSet[int]'
$queue = New-Object 'System.Collections.Generic.Queue[object]'
foreach ($root in $roots) {
  $since = [DateTimeOffset]::FromUnixTimeMilliseconds($root[1]).LocalDateTime
  $self = $all | Where-Object { $_.ProcessId -eq $root[0] -and $_.CreationDate -ge $since.AddSeconds(-5) -and $_.CreationDate -le $since.AddSeconds(5) }
  if ($self) { [void]$doomed.Add($root[0]) }
  $queue.Enqueue(@($root[0], $since.AddSeconds(-5)))
}
while ($queue.Count -gt 0) {
  $parent, $since = $queue.Dequeue()
  foreach ($process in $all) {
    if ($process.ParentProcessId -eq $parent -and $process.CreationDate -ge $since -and $doomed.Add([int]$process.ProcessId)) {
      $queue.Enqueue(@([int]$process.ProcessId, $process.CreationDate))
    }
  }
}
foreach ($id in $doomed) { Stop-Process -Id $id -Force }
`;

/** Ends every launched root and all of its descendants, alive or orphaned. */
export function killTrees() {
  if (roots.length === 0) return;
  if (process.platform === "win32") {
    const list = roots.map(
      ({ pid, started }) => `$roots += ,@(${pid}, ${started});`,
    );
    spawnSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        `$roots = @(); ${list.join(" ")} ${WINDOWS_SWEEP}`,
      ],
      { stdio: "ignore", windowsHide: true },
    );
    return;
  }
  for (const { pid } of roots) {
    try {
      process.kill(-pid, "SIGKILL");
    } catch {
      // The group is gone.
    }
  }
}

/** Asks the roots to stop: POSIX groups get SIGTERM; Windows has no gentle signal. */
export function signalRoots() {
  if (process.platform === "win32") return;
  for (const { pid } of roots) {
    try {
      process.kill(-pid, "SIGTERM");
    } catch {
      // The group is gone.
    }
  }
}
