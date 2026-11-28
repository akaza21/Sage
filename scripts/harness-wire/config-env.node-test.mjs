import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import * as claude from "./claude.mjs";
import * as codex from "./codex.mjs";
import {
  assertEmptyHome,
  configRoots,
  freshScratch,
  scenarioEnv,
} from "./config-env.mjs";
import * as pi from "./pi.mjs";
import { makeSanitizer } from "./sanitize.mjs";

const drivers = { claude, codex, pi };
// An owner's environment: a config dir of their own, sign-ins any harness
// might read (one no harness knows of yet), and what a process needs.
const owner = (driver) => ({
  [driver.CONFIG.env]: "/home/owner/config",
  ANTHROPIC_API_KEY: "secret",
  CLAUDE_CODE_OAUTH_TOKEN: "secret",
  OPENAI_API_KEY: "secret",
  CODEX_API_KEY: "secret",
  GROQ_API_KEY: "secret",
  SOME_FUTURE_PROVIDER_API_KEY: "secret",
  PATH: "/usr/bin",
  Path: "C:\\Windows",
  SystemRoot: "C:\\Windows",
});

for (const [harness, driver] of Object.entries(drivers)) {
  test(`${harness}: a signed-in scenario runs on the owner's own sign-in, never a copy`, () => {
    for (const scenario of driver.SCENARIOS.filter(
      (name) => name !== "auth-fail",
    )) {
      const env = scenarioEnv(
        driver.CONFIG,
        scenario,
        owner(driver),
        "/scratch/empty-home",
      );
      assert.equal(env[driver.CONFIG.env], "/home/owner/config", scenario);
      assert.equal(env.OPENAI_API_KEY, "secret", scenario);
    }
  });

  test(`${harness}: the signed-out scenario carries no sign-in of any name`, () => {
    const env = scenarioEnv(
      driver.CONFIG,
      "auth-fail",
      owner(driver),
      "/scratch/empty-home",
    );
    assert.equal(env[driver.CONFIG.env], "/scratch/empty-home");
    assert.deepEqual(
      Object.keys(env).filter((name) => /KEY|TOKEN|SECRET/i.test(name)),
      [],
    );
    assert.equal(env.PATH, "/usr/bin");
    assert.equal(env.SystemRoot, "C:\\Windows");
  });
}

test("a config dir the owner keeps outside the home folder never reaches a capture", () => {
  const config = path.join(os.tmpdir(), "private-codex-home");
  const sanitizer = makeSanitizer({
    roots: configRoots(codex.CONFIG, { CODEX_HOME: config }),
  });
  const skill = path.join(config, "skills", "mine", "SKILL.md");
  assert.doesNotMatch(sanitizer.text(skill), /private-codex-home/);
});

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

test("only a scratch dir the capture made is ever cleared", () => {
  const work = fs.mkdtempSync(path.join(os.tmpdir(), "sage-scratch-"));
  try {
    const foreign = path.join(work, "pi-auth-fail");
    fs.mkdirSync(path.join(foreign, "empty-home"), { recursive: true });
    fs.writeFileSync(path.join(foreign, "empty-home", "auth.json"), "{}");
    assert.throws(() => freshScratch(foreign), /not made by the capture/);
    assert.ok(fs.existsSync(path.join(foreign, "empty-home", "auth.json")));
    const ours = path.join(work, "pi-shell");
    freshScratch(ours);
    fs.writeFileSync(path.join(ours, "leftover"), "");
    freshScratch(ours);
    assert.equal(fs.existsSync(path.join(ours, "leftover")), false);
    // The owner pointed a harness at a home inside it and signed in there.
    const live = path.join(ours, "empty-home");
    fs.mkdirSync(live);
    fs.writeFileSync(path.join(live, "auth.json"), "{}");
    assert.throws(
      () => freshScratch(ours, [live]),
      /holds the harness config dir/,
    );
    assert.ok(fs.existsSync(path.join(live, "auth.json")));
  } finally {
    fs.rmSync(work, { recursive: true, force: true });
  }
});
