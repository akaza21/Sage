// @vitest-environment node
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  statSync,
  symlinkSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, expect, it } from "vitest";
import {
  devStateDirectory,
  ensurePrivateDirectory,
} from "./lib/dev-live-state.mjs";

const scratch: string[] = [];
afterEach(() => {
  for (const directory of scratch.splice(0))
    rmSync(directory, { recursive: true, force: true });
});

function scratchDirectory() {
  const directory = mkdtempSync(path.join(tmpdir(), "sage-dev-state-"));
  scratch.push(directory);
  return directory;
}

it("keeps POSIX state in the user's runtime or cache directory, never the shared temp folder", () => {
  const checkout = "/work/sage";
  const runtime = devStateDirectory(checkout, {
    XDG_RUNTIME_DIR: "/run/user/1000",
  });
  const cache = devStateDirectory(checkout, {});
  if (process.platform === "win32") {
    expect(runtime.base).toBe(cache.base);
    return;
  }
  expect(runtime.base).toBe("/run/user/1000/sage-dev");
  expect(cache.base).toMatch(/[/]\.cache[/]sage-dev$/u);
  expect(cache.directory.startsWith(`${cache.base}/sage-`)).toBe(true);
});

// POSIX permissions and ownership: Windows has neither mode bits nor symlink
// rights for ordinary users, and its state lives in the per-user profile.
const posix = { skip: process.platform === "win32" };

it("creates the state directory for this user alone", posix, () => {
  const directory = path.join(scratchDirectory(), "sage-dev");
  ensurePrivateDirectory(directory);
  expect(statSync(directory).mode & 0o777).toBe(0o700);
  ensurePrivateDirectory(directory);
});

it("refuses a state directory other users can open", posix, () => {
  const directory = path.join(scratchDirectory(), "sage-dev");
  mkdirSync(directory, { mode: 0o700 });
  chmodSync(directory, 0o755);
  expect(() => ensurePrivateDirectory(directory)).toThrow(
    "is open to other users",
  );
});

it("refuses a state directory planted as a symbolic link", posix, () => {
  const parent = scratchDirectory();
  const target = path.join(parent, "elsewhere");
  mkdirSync(target, { mode: 0o700 });
  const directory = path.join(parent, "sage-dev");
  symlinkSync(target, directory);
  expect(() => ensurePrivateDirectory(directory)).toThrow("is a symbolic link");
});
