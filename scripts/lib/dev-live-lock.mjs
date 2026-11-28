// One `npm run dev` per checkout: the launcher holds this lock before it
// touches the shared throwaway profile or its run record, so a second launch
// refuses instead of deleting the first one's run.
import { closeSync, openSync, readSync, rmSync, writeSync } from "node:fs";
import path from "node:path";

const OWNER_BYTES = 32;

function readOwner(file) {
  let descriptor;
  try {
    descriptor = openSync(file, "r");
    const buffer = Buffer.alloc(OWNER_BYTES);
    const bytes = readSync(descriptor, buffer, 0, OWNER_BYTES, 0);
    const text = buffer.toString("utf8", 0, bytes).trim();
    return /^\d{1,10}$/.test(text) ? Number(text) : undefined;
  } catch {
    return undefined;
  } finally {
    if (descriptor !== undefined) closeSync(descriptor);
  }
}

function isAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error.code === "EPERM";
  }
}

/**
 * Takes `<directory>/launcher.lock` for this process and returns its release.
 * A lock whose owner has exited is stale and is taken over.
 */
export function acquireLauncherLock(directory) {
  const file = path.join(directory, "launcher.lock");
  for (let attempt = 0; attempt < 2; attempt++) {
    let descriptor;
    try {
      descriptor = openSync(file, "wx");
    } catch (error) {
      if (error.code !== "EEXIST") throw error;
      const owner = readOwner(file);
      if (owner !== undefined && owner !== process.pid && isAlive(owner))
        throw new Error(
          `npm run dev is already running in this checkout (process ${owner}). ` +
            `Stop it first; if no such launcher exists, delete ${file}.`,
          { cause: error },
        );
      rmSync(file, { force: true });
      continue;
    }
    writeSync(descriptor, `${process.pid}\n`);
    closeSync(descriptor);
    return () => {
      if (readOwner(file) === process.pid) rmSync(file, { force: true });
    };
  }
  throw new Error(`Could not take the launcher lock ${file}.`);
}
