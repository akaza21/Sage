// The launcher's children and everything they start (cargo, rustc, the debug
// app and its harnesses) stop together. Each root is remembered for the
// launcher's lifetime, even after it exits, so its descendants stay reachable.
import { spawn, spawnSync } from "node:child_process";

const roots = [];

export function launch(args, { cwd, env }) {
  // The child's creation time lies between these two clock readings, which
  // is how the Windows sweep tells this process from a later one given the
  // same id.
  const before = Date.now();
  const child = spawn(process.execPath, args, {
    cwd,
    env,
    stdio: ["ignore", "inherit", "inherit"],
    // POSIX: its own process group, which outlives an exited leader.
    detached: process.platform !== "win32",
  });
  if (child.pid !== undefined) {
    const root = { pid: child.pid, before, after: Date.now(), exited: 0 };
    // Its orphans were all started before it exited.
    child.once("exit", () => (root.exited = Date.now()));
    roots.push(root);
  }
  return child;
}

// Windows keeps a process's parent id after the parent exits, so the tree is
// rebuilt from one process snapshot:
// - a root is itself only if its creation time falls in its launch window;
// - a live process's children are the processes naming it as parent;
// - an exited root's orphans name its id and were created after it started,
//   before it exited (plus a second of slack) and, if a newer process now
//   holds that id, before that process started.
// Not reached: orphans of an exited process below a root, and processes
// started after the snapshot.
const WINDOWS_SWEEP = `
$ErrorActionPreference = 'SilentlyContinue'
$all = @(Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, CreationDate)
$byId = @{}
foreach ($process in $all) { $byId[[int]$process.ProcessId] = $process }
$doomed = New-Object 'System.Collections.Generic.HashSet[int]'
$queue = New-Object 'System.Collections.Generic.Queue[object]'
foreach ($root in $roots) {
  $from = [DateTimeOffset]::FromUnixTimeMilliseconds($root[1] - 50).LocalDateTime
  $to = [DateTimeOffset]::FromUnixTimeMilliseconds($root[2] + 50).LocalDateTime
  $holder = $byId[[int]$root[0]]
  if ($holder -and $holder.CreationDate -ge $from -and $holder.CreationDate -le $to) {
    [void]$doomed.Add([int]$root[0])
    $queue.Enqueue(@([int]$root[0], $holder.CreationDate, [datetime]::MaxValue))
  } else {
    $until = if ($root[3] -gt 0) { [DateTimeOffset]::FromUnixTimeMilliseconds($root[3] + 1000).LocalDateTime } else { [datetime]::MaxValue }
    if ($holder -and $holder.CreationDate -lt $until) { $until = $holder.CreationDate }
    $queue.Enqueue(@([int]$root[0], $from, $until))
  }
}
while ($queue.Count -gt 0) {
  $parent, $from, $until = $queue.Dequeue()
  foreach ($process in $all) {
    if ($process.ParentProcessId -eq $parent -and $process.CreationDate -ge $from -and $process.CreationDate -lt $until -and $doomed.Add([int]$process.ProcessId)) {
      $queue.Enqueue(@([int]$process.ProcessId, $process.CreationDate, [datetime]::MaxValue))
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
      ({ pid, before, after, exited }) =>
        `$roots += ,@(${pid}, ${before}, ${after}, ${exited});`,
    );
    spawnSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        `$roots = @(); ${list.join(" ")} ${WINDOWS_SWEEP}`,
      ],
      // A stuck snapshot must not hold the launcher open.
      { stdio: "ignore", windowsHide: true, timeout: 10_000 },
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
