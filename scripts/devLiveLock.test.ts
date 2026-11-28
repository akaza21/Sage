// @vitest-environment node
import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
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

/** A launcher that waits for `go`, tries the lock, reports, then holds a while. */
function contender(directory: string, go: string) {
  const launcher = spawn(
    process.execPath,
    [
      "--input-type=module",
      "-e",
      `const { existsSync } = await import("node:fs");
       const { acquireLauncherLock } = await import(${JSON.stringify(lockModule)});
       while (!existsSync(${JSON.stringify(go)})) await new Promise((r) => setTimeout(r, 5));
       try { acquireLauncherLock(${JSON.stringify(directory)}); console.log("won"); }
       catch { console.log("refused"); }
       setTimeout(() => {}, 1500);`,
    ],
    { stdio: ["ignore", "pipe", "inherit"] },
  );
  launchers.push(launcher);
  return new Promise<string>((resolve) =>
    launcher.stdout!.once("data", (data: Buffer) =>
      resolve(data.toString().trim()),
    ),
  );
}

it("lets exactly one of two launchers take over a stale lock at once", async () => {
  // A process id that no longer runs.
  const gone = spawn(process.execPath, ["-e", ""]);
  await new Promise((exited) => gone.once("exit", exited));
  for (let round = 0; round < 8; round++) {
    const directory = mkdtempSync(path.join(tmpdir(), "sage-dev-lock-"));
    directories.push(directory);
    writeFileSync(path.join(directory, "launcher.lock"), `${gone.pid} 1\n`);
    const go = path.join(directory, "go");
    const outcomes = [contender(directory, go), contender(directory, go)];
    await new Promise((settle) => setTimeout(settle, 400));
    writeFileSync(go, "");
    expect((await Promise.all(outcomes)).sort()).toEqual(["refused", "won"]);
  }
}, 60_000);
