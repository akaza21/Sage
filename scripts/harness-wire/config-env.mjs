// How a capture scenario reaches a harness's sign-in, and the scratch space it
// may clear. A signed-in scenario runs on the harness's own config dir, as
// Sage runs it, so a token the harness refreshes is written back to the
// owner's real sign-in. A sign-in is never copied: a refresh inside a copy
// rotates the token and leaves the owner's file stale (it signed Pi out on
// 2026-11-28). Only `auth-fail` runs signed out, on an empty home with an
// environment reduced to what a process needs to start.
import fs from "node:fs";
import path from "node:path";

export const SIGNED_OUT = "auth-fail";

// What a harness needs to start, and nothing that can carry a sign-in.
const ESSENTIALS = new Set(
  [
    "PATH",
    "PATHEXT",
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "WINDIR",
    "COMSPEC",
    "TEMP",
    "TMP",
    "TMPDIR",
    "HOME",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "PROGRAMDATA",
    "PROGRAMFILES",
    "PROGRAMFILES(X86)",
    "COMMONPROGRAMFILES",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    "OS",
    "LANG",
    "LC_ALL",
    "USER",
    "USERNAME",
    "SHELL",
  ].map((name) => name.toUpperCase()),
);

// `config`: the driver's `{ env, extra }`: the variable naming its config dir,
// and fixed settings.
export function scenarioEnv(config, scenario, env, emptyHome) {
  if (scenario !== SIGNED_OUT) return { ...env, ...config.extra };
  const essentials = Object.fromEntries(
    Object.entries(env).filter(([name]) => ESSENTIALS.has(name.toUpperCase())),
  );
  return { ...essentials, ...config.extra, [config.env]: emptyHome };
}

// A signed-out home must hold nothing, least of all a sign-in.
export function assertEmptyHome(dir) {
  const found = fs.readdirSync(dir);
  if (found.length)
    throw new Error(
      `${dir} must be empty before a signed-out run: ${found.join(", ")}`,
    );
}

// The harness's own config dir, when the owner set it outside the home
// folder, as a sanitizer root, so a path under it never reaches a capture.
export function configRoots(config, env) {
  return env[config.env] ? [[env[config.env], "config"]] : [];
}

const MARKER = ".sage-capture-scratch";

// Clears and recreates a scenario's scratch dir. Only a dir this tool made
// (it holds the marker) is ever cleared; anything else there is refused.
export function freshScratch(dir) {
  if (fs.existsSync(dir) && fs.readdirSync(dir).length) {
    if (!fs.existsSync(path.join(dir, MARKER)))
      throw new Error(
        `${dir} was not made by the capture; refusing to clear it`,
      );
    fs.rmSync(dir, { recursive: true, force: true });
  }
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, MARKER), "");
}
