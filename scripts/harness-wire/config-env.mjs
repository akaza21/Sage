// How a capture scenario reaches a harness's sign-in. A signed-in scenario
// runs on the harness's own config dir, as Sage runs it, so a token the
// harness refreshes is written back to the owner's real sign-in. A sign-in
// is never copied: a refresh inside a copy rotates the token and leaves the
// owner's file stale (it signed Pi out on 2026-11-28). Only `auth-fail` runs
// signed out, on an empty home with the sign-in variables removed.
import fs from "node:fs";

export const SIGNED_OUT = "auth-fail";

// `config`: the driver's `{ env, keys, extra }`: the variable naming its
// config dir, the variables that carry a sign-in, and fixed settings.
export function scenarioEnv(config, scenario, env, emptyHome) {
  const scenarioEnv = { ...env, ...config.extra };
  if (scenario !== SIGNED_OUT) return scenarioEnv;
  scenarioEnv[config.env] = emptyHome;
  for (const key of config.keys) delete scenarioEnv[key];
  return scenarioEnv;
}

// A signed-out home must hold nothing, least of all a sign-in.
export function assertEmptyHome(dir) {
  const found = fs.readdirSync(dir);
  if (found.length)
    throw new Error(
      `${dir} must be empty before a signed-out run: ${found.join(", ")}`,
    );
}
