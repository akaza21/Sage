// Where `npm run dev` keeps its per-checkout state (throwaway profile, run
// record with the pairing code, launcher lock): outside the checkout, which is
// the dev server's root, and private to the current user.
import { createHash } from "node:crypto";
import { lstatSync, mkdirSync } from "node:fs";
import os from "node:os";
import path from "node:path";

/**
 * The user's runtime directory if set, else ~/.cache: never a shared temp
 * folder, where another user could read the run record or plant a directory
 * first. Not %LOCALAPPDATA% on Windows either: a packaged (MSIX) app such as
 * an agent host redirects its processes' writes there into a private copy,
 * so the launcher, the dev server and the debug app could each see a
 * different run record.
 */
export function devStateDirectory(checkout, env = process.env) {
  const windows = process.platform === "win32";
  const base =
    !windows && env.XDG_RUNTIME_DIR
      ? path.join(env.XDG_RUNTIME_DIR, "sage-dev")
      : path.join(os.homedir(), ".cache", "sage-dev");
  const key = createHash("sha256")
    .update(windows ? checkout.toLowerCase() : checkout)
    .digest("hex")
    .slice(0, 16);
  return {
    base,
    directory: path.join(base, `${path.basename(checkout)}-${key}`),
  };
}

/**
 * Creates `directory` for this user alone (0700) or checks an existing one:
 * on POSIX it must be a real directory, owned by this user, with no group or
 * other access. Throws otherwise.
 */
export function ensurePrivateDirectory(directory) {
  try {
    mkdirSync(directory, { mode: 0o700 });
  } catch (error) {
    if (error.code !== "EEXIST") throw error;
  }
  if (process.platform === "win32") return;
  const stat = lstatSync(directory);
  const problem = stat.isSymbolicLink()
    ? "is a symbolic link"
    : !stat.isDirectory()
      ? "is not a directory"
      : stat.uid !== process.getuid()
        ? "belongs to another user"
        : (stat.mode & 0o077) !== 0
          ? "is open to other users"
          : undefined;
  if (problem)
    throw new Error(
      `${directory} ${problem}; npm run dev keeps its pairing code there and will not use it. Remove it or make it private (chmod 700).`,
    );
}
