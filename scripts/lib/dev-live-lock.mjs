// One `npm run dev` per checkout: the launcher holds this lock before it
// touches the throwaway profile or its run record, so a second launch refuses
// instead of deleting the first one's run.
//
// The lock is created whole and atomically (a hard link of a written file),
// so it never exists empty. A lock whose owner has exited is stale; taking it
// over happens under a short takeover lock, so two launchers that both judged
// it stale cannot both remove it and one of them never deletes a fresh lock.
import { randomBytes } from "node:crypto";
import {
  closeSync,
  linkSync,
  openSync,
  readSync,
  rmSync,
  statSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";

const OWNER_BYTES = 128;
// A lock or takeover file this young may still be mid-write by its owner.
const SETTLE_MS = 5_000;
const TAKEOVER_ATTEMPTS = 100;
const TAKEOVER_WAIT_MS = 50;
const started = Math.round(Date.now() - process.uptime() * 1000);

function pause(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

/** "held" (by a live process or too young to judge), "stale", or "absent". */
function inspect(file) {
  let descriptor;
  let text;
  try {
    descriptor = openSync(file, "r");
    const buffer = Buffer.alloc(OWNER_BYTES);
    text = buffer.toString(
      "utf8",
      0,
      readSync(descriptor, buffer, 0, OWNER_BYTES, 0),
    );
  } catch (error) {
    if (error.code === "ENOENT") return { state: "absent" };
    text = "";
  } finally {
    if (descriptor !== undefined) closeSync(descriptor);
  }
  const match = /^(\d{1,10}) (\d{1,15})\n$/.exec(text);
  if (!match) {
    let age;
    try {
      age = Date.now() - statSync(file).mtimeMs;
    } catch {
      return { state: "absent" };
    }
    return { state: age < SETTLE_MS ? "held" : "stale" };
  }
  const owner = Number(match[1]);
  if (owner === process.pid && Number(match[2]) === started)
    return { state: "held", owner, mine: true };
  try {
    process.kill(owner, 0);
    return { state: "held", owner };
  } catch (error) {
    return error.code === "EPERM"
      ? { state: "held", owner }
      : { state: "stale" };
  }
}

/** Creates `file` holding `body`, failing with EEXIST if it already exists. */
function createWhole(file, body) {
  const staged = `${file}.${process.pid}-${randomBytes(6).toString("hex")}`;
  writeFileSync(staged, body, { flag: "wx" });
  try {
    linkSync(staged, file);
  } finally {
    rmSync(staged, { force: true });
  }
}

/** Removes `file` if it is still stale, under the takeover lock. */
function removeStale(file) {
  const takeover = `${file}.takeover`;
  for (let attempt = 0; attempt < TAKEOVER_ATTEMPTS; attempt++) {
    try {
      createWhole(takeover, `${process.pid} ${started}\n`);
    } catch (error) {
      if (error.code !== "EEXIST") throw error;
      // A taker that crashed mid-takeover leaves a file that never changes.
      if (inspect(takeover).state === "stale")
        rmSync(takeover, { force: true });
      else pause(TAKEOVER_WAIT_MS);
      continue;
    }
    try {
      // Only takers delete the lock, and they are serialized here, so a lock
      // still judged stale now cannot be replaced before it is removed.
      if (inspect(file).state === "stale") unlinkSync(file);
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    } finally {
      unlinkSync(takeover);
    }
    return;
  }
  throw new Error(`Another launcher kept ${takeover} busy.`);
}

/**
 * Takes `<directory>/launcher.lock` for this process and returns its release.
 * Throws if another live launcher holds it.
 */
export function acquireLauncherLock(directory) {
  const file = path.join(directory, "launcher.lock");
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      createWhole(file, `${process.pid} ${started}\n`);
      return () => {
        if (inspect(file).mine) rmSync(file, { force: true });
      };
    } catch (error) {
      if (error.code !== "EEXIST") throw error;
      const lock = inspect(file);
      if (lock.state === "held")
        throw new Error(
          lock.owner === undefined
            ? `Another npm run dev is starting in this checkout (${file}).`
            : `npm run dev is already running in this checkout (process ${lock.owner}). ` +
                `Stop it first; if no such launcher exists, delete ${file}.`,
          { cause: error },
        );
      if (lock.state === "stale") removeStale(file);
    }
  }
  throw new Error(`Could not take the launcher lock ${file}.`);
}
