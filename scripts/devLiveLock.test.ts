// @vitest-environment node
import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { afterEach, expect, it } from "vitest";
import { acquireLauncherLock } from "./lib/dev-live-lock.mjs";

const lockModule = pathToFileURL(
  path.resolve("scripts/lib/dev-live-lock.mjs"),
).href;
const directories: string[] = [];
const launchers: ChildProcess[] = [];

afterEach(() => {
  for (const launcher of launchers.splice(0)) launcher.kill();
  for (const directory of directories.splice(0))
    rmSync(directory, { recursive: true, force: true });
});

/** Another launcher process that takes the lock and keeps running. */
async function firstLauncher(directory: string) {
  const launcher = spawn(
    process.execPath,
    [
      "--input-type=module",
      "-e",
      `const { acquireLauncherLock } = await import(${JSON.stringify(lockModule)});
       acquireLauncherLock(${JSON.stringify(directory)});
       console.log("locked");
       setInterval(() => {}, 1000);`,
    ],
    { stdio: ["ignore", "pipe", "inherit"] },
  );
  launchers.push(launcher);
  await new Promise<void>((resolve, reject) => {
    launcher.stdout!.once("data", () => resolve());
    launcher.once("exit", () => reject(new Error("first launcher exited")));
  });
  return launcher;
}

it("refuses a second launcher while the first runs, and takes over once it is gone", async () => {
  const directory = mkdtempSync(path.join(tmpdir(), "sage-dev-lock-"));
  directories.push(directory);
  const first = await firstLauncher(directory);

  expect(() => acquireLauncherLock(directory)).toThrow(
    `npm run dev is already running in this checkout (process ${first.pid})`,
  );
  expect(existsSync(path.join(directory, "launcher.lock"))).toBe(true);

  first.kill();
  await new Promise((exited) => first.once("exit", exited));
  const release = acquireLauncherLock(directory);
  release();
  expect(existsSync(path.join(directory, "launcher.lock"))).toBe(false);
});
