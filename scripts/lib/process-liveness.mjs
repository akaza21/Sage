import { readdirSync, readFileSync } from "node:fs";

// `kill(pid, 0)` succeeds for a zombie: a process that has exited but whose
// parent has not reaped it yet. Containers whose PID 1 never reaps orphans
// (no init) keep such zombies forever, so on Linux a zombie counts as exited.
// Elsewhere there is no cheap state probe and `kill(pid, 0)` stays the answer.

function linuxState(pid) {
  try {
    const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
    // The command name is parenthesised and may itself contain ") ".
    const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
    return { state: fields[0], processGroupId: Number(fields[2]) };
  } catch {
    return null;
  }
}

export function processAlive(pid) {
  try {
    process.kill(pid, 0);
  } catch (error) {
    return error.code !== "ESRCH";
  }
  if (process.platform !== "linux") return true;
  const status = linuxState(pid);
  // An unreadable entry fails closed: the process is treated as running.
  return status === null || status.state !== "Z";
}

export function posixGroupAlive(processGroupId) {
  try {
    process.kill(-processGroupId, 0);
  } catch (error) {
    if (error.code === "ESRCH") return false;
    if (error.code === "EPERM") return true;
    throw error;
  }
  if (process.platform !== "linux") return true;
  let entries;
  try {
    entries = readdirSync("/proc");
  } catch {
    return true;
  }
  for (const entry of entries) {
    if (!/^\d+$/u.test(entry)) continue;
    const status = linuxState(Number(entry));
    if (status?.processGroupId === processGroupId && status.state !== "Z") {
      return true;
    }
  }
  // Every member that kill() still reached is a zombie: the group has exited.
  return false;
}
