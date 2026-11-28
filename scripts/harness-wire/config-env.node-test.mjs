import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import * as claude from "./claude.mjs";
import * as codex from "./codex.mjs";
import { assertEmptyHome, scenarioEnv } from "./config-env.mjs";
import * as pi from "./pi.mjs";

const drivers = { claude, codex, pi };

for (const [harness, driver] of Object.entries(drivers)) {
  test(`${harness}: a signed-in scenario runs on the owner's own sign-in, never a copy`, () => {
    const owner = {
      [driver.CONFIG.env]: "/home/owner/config",
      ...Object.fromEntries(driver.CONFIG.keys.map((key) => [key, "set"])),
    };
    for (const scenario of driver.SCENARIOS.filter(
      (name) => name !== "auth-fail",
    )) {
      const env = scenarioEnv(
        driver.CONFIG,
        scenario,
        owner,
        "/scratch/empty-home",
      );
      assert.equal(env[driver.CONFIG.env], "/home/owner/config", scenario);
      for (const key of driver.CONFIG.keys) assert.equal(env[key], "set", key);
    }
  });

  test(`${harness}: the signed-out scenario has no sign-in at all`, () => {
    const owner = Object.fromEntries(
      driver.CONFIG.keys.map((key) => [key, "set"]),
    );
    const env = scenarioEnv(
      driver.CONFIG,
      "auth-fail",
      owner,
      "/scratch/empty-home",
    );
    assert.equal(env[driver.CONFIG.env], "/scratch/empty-home");
    for (const key of driver.CONFIG.keys) assert.equal(key in env, false, key);
  });
}

test("a signed-out home holding a sign-in file is refused", () => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "sage-empty-home-"));
  try {
    assertEmptyHome(home);
    for (const name of [".credentials.json", "auth.json"]) {
      fs.writeFileSync(path.join(home, name), "{}");
      assert.throws(
        () => assertEmptyHome(home),
        new RegExp(name.replace(".", "\\.")),
      );
      fs.rmSync(path.join(home, name));
    }
  } finally {
    fs.rmSync(home, { recursive: true, force: true });
  }
});
