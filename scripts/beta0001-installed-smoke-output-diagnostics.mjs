import { spawnSync } from "node:child_process";

const phases = [
  "script-start",
  "identity-start",
  "identity-verified",
  "compile-start",
  "compile-complete",
  "seal-start",
  "seal-acquired",
  "ready-written",
];

// Fixed labels and numeric accounting only: never include paths, requests,
// environment values or arbitrary child output in the diagnostic.
export const guardianPhaseSource = String.raw`
$authorityClock = [Diagnostics.Stopwatch]::StartNew()
$authorityProcess = [Diagnostics.Process]::GetCurrentProcess()
function Write-AuthorityPhase([string]$phase) {
  $authorityProcess.Refresh()
  [Console]::Error.WriteLine(('sage-output-authority-phase {0} {1} {2} {3}' -f
    $phase, $authorityClock.ElapsedMilliseconds,
    [long]$authorityProcess.UserProcessorTime.TotalMilliseconds,
    [long]$authorityProcess.PrivilegedProcessorTime.TotalMilliseconds))
  [Console]::Error.Flush()
}
Write-AuthorityPhase 'script-start'
`;

function sampleCpu(child, powerShell) {
  if (process.platform !== "win32" || !powerShell || !child.pid) {
    return { status: "unavailable" };
  }
  // This read-only sample is failure-only and independently bounded. In
  // particular, a stalled PowerShell image must not stall diagnostics too.
  // Retain the queried process object while reading its accounting.
  const source = `$ErrorActionPreference = 'Stop'
    $p = [Diagnostics.Process]::GetProcessById(${child.pid})
    $p.Refresh()
    @{pid=$p.Id; userMs=[long]$p.UserProcessorTime.TotalMilliseconds;
      kernelMs=[long]$p.PrivilegedProcessorTime.TotalMilliseconds} | ConvertTo-Json -Compress`;
  const result = spawnSync(
    powerShell.path,
    [
      "-NoLogo",
      "-NoProfile",
      "-NonInteractive",
      "-EncodedCommand",
      Buffer.from(source, "utf16le").toString("base64"),
    ],
    { windowsHide: true, timeout: 1500, maxBuffer: 1024, encoding: "utf8" },
  );
  if (result.error || result.status !== 0) return { status: "unavailable" };
  try {
    const value = JSON.parse(result.stdout);
    if (
      value.pid === child.pid &&
      [value.userMs, value.kernelMs].every(
        (number) => Number.isSafeInteger(number) && number >= 0,
      )
    ) {
      return {
        status: "sampled-by-pid",
        userMs: value.userMs,
        kernelMs: value.kernelMs,
      };
    }
  } catch {
    /* A malformed sample cannot replace the startup failure. */
  }
  return { status: "unavailable" };
}

export function observeGuardianStartup(child) {
  const startedAt = new Date().toISOString();
  const started = performance.now();
  const elapsed = () => Math.round(performance.now() - started);
  const observedPhases = Object.fromEntries(
    phases.map((phase) => [phase, null]),
  );
  const stdout = { bytes: 0, ended: false, firstDataMs: null };
  const stderr = { bytes: 0, ended: false, firstDataMs: null };
  const events = { spawnMs: null, exitMs: null, closeMs: null };
  let line = "";
  let oversizedLine = false;
  const listeners = [];
  function listen(emitter, event, handler) {
    emitter.on(event, handler);
    listeners.push(() => emitter.off(event, handler));
  }
  for (const [stream, state] of [
    [child.stdout, stdout],
    [child.stderr, stderr],
  ]) {
    listen(stream, "end", () => {
      state.ended = true;
    });
    listen(stream, "data", (chunk) => {
      state.bytes += chunk.byteLength;
      state.firstDataMs ??= elapsed();
      if (stream !== child.stderr) return;
      for (const byte of chunk) {
        if (byte === 0x0a) {
          const match =
            !oversizedLine &&
            line.match(
              /^sage-output-authority-phase ([a-z-]+) (\d+) (\d+) (\d+)\r?$/u,
            );
          if (match && phases.includes(match[1])) {
            const values = match.slice(2).map(Number);
            if (values.every(Number.isSafeInteger)) {
              observedPhases[match[1]] = {
                elapsedMs: values[0],
                userMs: values[1],
                kernelMs: values[2],
                observedMs: elapsed(),
              };
            }
          }
          line = "";
          oversizedLine = false;
        } else if (line.length < 160) {
          line += String.fromCharCode(byte);
        } else {
          oversizedLine = true;
        }
      }
    });
  }
  listen(child, "spawn", () => {
    events.spawnMs = elapsed();
  });
  listen(child, "exit", () => {
    events.exitMs = elapsed();
  });
  listen(child, "close", () => {
    events.closeMs = elapsed();
  });
  function snapshot() {
    return {
      startedAt,
      elapsedMs: elapsed(),
      authority: "ready-not-observed",
      child: {
        pid: child.pid ?? null,
        state:
          child.exitCode !== null || child.signalCode !== null
            ? "exited"
            : "running",
        exitCode: child.exitCode,
        signalCode: child.signalCode,
      },
      events: { ...events },
      stdout: { ...stdout },
      stderr: { ...stderr },
      phases: { ...observedPhases },
    };
  }
  return {
    snapshot,
    timeoutError(powerShell) {
      const diagnostic = snapshot();
      diagnostic.child.cpu = sampleCpu(child, powerShell);
      diagnostic.child.cpu.observedMs = elapsed();
      const error = new Error(
        `installed-smoke-output-authority-timeout ${JSON.stringify(diagnostic)}`,
      );
      error.diagnostic = diagnostic;
      return error;
    },
    dispose() {
      for (const remove of listeners) remove();
    },
  };
}
