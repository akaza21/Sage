import { spawn, spawnSync } from "node:child_process";
import { existsSync, lstatSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const CODE_LOADING_ENVIRONMENT_KEYS = new Set([
  "NODE_OPTIONS",
  "NODE_PATH",
  "NODE_PRESERVE_SYMLINKS",
  "NODE_PRESERVE_SYMLINKS_MAIN",
  "NODE_REPL_EXTERNAL_MODULE",
]);
const SAFE_CHILD_ENVIRONMENT_KEYS = [
  "ALLUSERSPROFILE",
  "APPDATA",
  "COMMONPROGRAMFILES",
  "COMMONPROGRAMFILES(X86)",
  "COMSPEC",
  "HOME",
  "HOMEDRIVE",
  "HOMEPATH",
  "LANG",
  "LC_ALL",
  "LC_CTYPE",
  "LOCALAPPDATA",
  "OS",
  "PATH",
  "PATHEXT",
  "PROGRAMDATA",
  "PROGRAMFILES",
  "PROGRAMFILES(X86)",
  "PUBLIC",
  "SYSTEMDRIVE",
  "SYSTEMROOT",
  "TEMP",
  "TMP",
  "TMPDIR",
  "TZ",
  "USERDOMAIN",
  "USERNAME",
  "USERPROFILE",
  "WINDIR",
];

function fail(code) {
  throw new Error(`journey-${code}`);
}

function samePath(left, right) {
  const a = path.resolve(left);
  const b = path.resolve(right);
  return process.platform === "win32"
    ? a.toLowerCase() === b.toLowerCase()
    : a === b;
}

function controlledChildEnvironment(ambient = process.env) {
  const byUppercaseName = new Map(
    Object.entries(ambient).map(([name, value]) => [name.toUpperCase(), value]),
  );
  for (const name of CODE_LOADING_ENVIRONMENT_KEYS)
    if ((byUppercaseName.get(name) ?? "").length > 0)
      fail("tool-environment-invalid");
  return Object.fromEntries(
    SAFE_CHILD_ENVIRONMENT_KEYS.flatMap((name) => {
      const value = byUppercaseName.get(name);
      return value === undefined ? [] : [[name, value]];
    }),
  );
}

export function processIdentityHelper(action, value) {
  const helper = fileURLToPath(
    new URL("./run-source-functional-native-ide-journey.ps1", import.meta.url),
  );
  const powershell = path.join(
    process.env.SystemRoot ?? "C:\\Windows",
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const args = [
    "-NoProfile",
    "-NonInteractive",
    "-File",
    helper,
    "-ToolIdentityAction",
    action,
  ];
  if (action === "capture") args.push("-ToolProcessId", String(value));
  else
    args.push(
      "-ToolIdentityJson",
      Buffer.from(JSON.stringify(value), "utf8").toString("base64"),
    );
  const result = spawnSync(powershell, args, {
    windowsHide: true,
    encoding: "utf8",
    timeout: 5000,
    maxBuffer: 64 * 1024,
  });
  if (result.error || result.status !== 0) fail("tool-cleanup-unproven");
  try {
    return JSON.parse(result.stdout.trim());
  } catch {
    fail("tool-cleanup-unproven");
  }
}

export function runBoundedTool(executable, args, deadline, cwd, deps = {}) {
  const spawnTool = deps.spawn ?? spawn;
  const maximumOutputBytes = deps.maxOutputBytes ?? 64 * 1024;
  const childEnvironment = controlledChildEnvironment();
  const remaining = deadline - Date.now();
  if (remaining <= 0) fail("deadline-exhausted");
  const hasIdentityHandshake = typeof deps.identityHandshake === "string";
  if (hasIdentityHandshake && deps.identityHandshake.length === 0)
    fail("tool-cleanup-unproven");
  return new Promise((resolve, reject) => {
    let child;
    try {
      child = spawnTool(executable, args, {
        cwd,
        env: childEnvironment,
        shell: false,
        windowsHide: true,
        stdio: [hasIdentityHandshake ? "pipe" : "ignore", "pipe", "pipe"],
      });
    } catch (error) {
      reject(
        new Error(`playwright-cli-spawn-${error?.code ?? "failed"}-before-pid`),
      );
      return;
    }
    if (!Number.isSafeInteger(child?.pid) || child.pid <= 0) {
      child?.once?.("error", () => {});
      reject(new Error("playwright-cli-spawn-without-pid"));
      return;
    }
    const chunks = [];
    let bytes = 0;
    let terminal = null;
    let identity;
    let settled = false;
    let settlementTimer = null;
    let timer = null;
    const quarantine = (category, cleanup = null) => {
      const record = {
        schemaVersion: 1,
        observedPid: child.pid,
        commandRole: String(deps.commandRole ?? "playwright-helper").slice(
          0,
          64,
        ),
        category,
        capturedAtUtc: new Date().toISOString(),
        deadlineUtc: new Date(deadline).toISOString(),
        cleanup: cleanup
          ? {
              identityLost: Boolean(cleanup.identityLost),
              remainingCount: Array.isArray(cleanup.remaining)
                ? cleanup.remaining.length
                : null,
            }
          : null,
      };
      if (deps.quarantine) deps.quarantine(record);
      else if (existsSync(cwd) && !lstatSync(cwd).isSymbolicLink()) {
        const target = path.join(
          cwd,
          `tool-cleanup-quarantine-${child.pid}-${Date.now()}.json`,
        );
        const encoded = `${JSON.stringify(record, null, 2)}\n`;
        if (Buffer.byteLength(encoded, "utf8") > 4096)
          fail("tool-cleanup-unproven");
        writeFileSync(target, encoded, { flag: "wx" });
      }
    };
    const finish = (code) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (settlementTimer) clearTimeout(settlementTimer);
      child.stdout.removeListener("data", take);
      child.stderr.removeListener("data", take);
      if (terminal || code !== 0)
        reject(new Error(`playwright-cli-${terminal ?? `exit-${code}`}`));
      else resolve(Buffer.concat(chunks).toString("utf8"));
    };
    const terminate = (reason) => {
      if (terminal) return;
      terminal = reason;
      let cleanup;
      try {
        cleanup = (
          deps.cleanupIdentity ??
          ((owned) => processIdentityHelper("cleanup", owned))
        )(identity);
      } catch {
        cleanup = null;
      }
      if (!cleanup || cleanup.identityLost || cleanup.remaining?.length !== 0) {
        terminal = "cleanup-unproven";
        quarantine(reason, cleanup);
      }
      settlementTimer = setTimeout(() => finish(null), 250);
    };
    const closeIdentityHandshake = (value) => {
      if (!hasIdentityHandshake) return;
      if (!child.stdin || child.stdin.destroyed || !child.stdin.writable)
        fail("tool-cleanup-unproven");
      child.stdin.end(value);
    };
    const take = (chunk) => {
      if (terminal) return;
      bytes += chunk.length;
      if (bytes > maximumOutputBytes) terminate("output-oversize");
      else chunks.push(Buffer.from(chunk));
    };
    child.stdout.on("data", take);
    child.stderr.on("data", take);
    try {
      identity = (
        deps.captureIdentity ?? ((pid) => processIdentityHelper("capture", pid))
      )(child.pid);
      if (
        Number(identity?.processId) !== child.pid ||
        !identity?.creationDate ||
        (deps.expectedExecutablePath &&
          (!identity?.executablePath ||
            !samePath(identity.executablePath, deps.expectedExecutablePath)))
      )
        fail("tool-cleanup-unproven");
      closeIdentityHandshake(deps.identityHandshake);
    } catch {
      terminal = "cleanup-unproven";
      try {
        closeIdentityHandshake();
      } catch {
        try {
          child.stdin?.destroy();
        } catch {
          // The command is already quarantined; do not hide its proof failure.
        }
      }
      quarantine("identity-capture-failed");
      settlementTimer = setTimeout(() => finish(null), 250);
    }
    child.on("error", (error) => terminate(`spawn-${error.code ?? "failed"}`));
    timer = setTimeout(() => terminate("timeout"), remaining);
    child.on("close", finish);
  });
}
