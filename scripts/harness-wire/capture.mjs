#!/usr/bin/env node
// Records each installed harness's wire through the fixed scenarios, sanitized,
// into <out>/<harness>/. Local tooling for the Omen; it spends real harness
// usage and is never part of a gate.
//
//   node scripts/harness-wire/capture.mjs --out <dir> [--harness claude,codex,pi]
//        [--scenario shell,edit] [--model <claude model>] [--work <scratch dir>]
//
// Then compare with the committed corpus:
//   node scripts/harness-wire/shape.mjs --diff src-tauri/tests/fixtures/harness-wire <dir>
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import * as claude from "./claude.mjs";
import * as codex from "./codex.mjs";
import * as pi from "./pi.mjs";
import { makeSanitizer } from "./sanitize.mjs";
import { shapeOfHarness } from "./shape.mjs";
import { twoColourPng } from "./wire.mjs";

export const MAX_TRANSCRIPT_BYTES = 96 * 1024;
const DRIVERS = { claude, codex, pi };
const VERSION_COMMANDS = { claude: "claude", codex: "codex", pi: "pi" };

function option(name, fallback) {
  const index = process.argv.indexOf(`--${name}`);
  return index >= 0 ? process.argv[index + 1] : fallback;
}

function version(harness) {
  try {
    return execFileSync(VERSION_COMMANDS[harness], ["--version"], {
      encoding: "utf8",
      shell: process.platform === "win32",
      windowsHide: true,
      timeout: 60000,
    })
      .trim()
      .split(/\r?\n/)
      .pop();
  } catch (error) {
    return `unknown (${error.message.split("\n")[0]})`;
  }
}

function scratchRepo(dir) {
  fs.rmSync(dir, { recursive: true, force: true });
  const repo = path.join(dir, "repo");
  fs.mkdirSync(path.join(repo, "src"), { recursive: true });
  fs.writeFileSync(path.join(repo, "README.md"), "# Wire capture scratch repo\n\nA disposable repository.\n");
  fs.writeFileSync(path.join(repo, "src", "greet.js"), "export function greet(name) {\n  return 'hello ' + name;\n}\n");
  const git = (...args) =>
    execFileSync("git", ["-c", "user.name=Capture", "-c", "user.email=capture@example.invalid", ...args], {
      cwd: repo,
      windowsHide: true,
    });
  git("init", "-q");
  git("add", ".");
  git("commit", "-q", "-m", "initial");
  const emptyHome = path.join(dir, "empty-home");
  fs.mkdirSync(emptyHome, { recursive: true });
  return { repo, emptyHome };
}

async function main() {
  const out = option("out");
  if (!out) throw new Error("--out <dir> is required");
  const harnesses = option("harness", "claude,codex,pi").split(",");
  const only = option("scenario", "")
    .split(",")
    .filter(Boolean);
  const model = option("model", undefined);
  const work = option("work", path.join(os.tmpdir(), "sage-wire-capture"));
  const image = await twoColourPng();
  for (const harness of harnesses) {
    const driver = DRIVERS[harness];
    if (!driver) throw new Error(`unknown harness ${harness}`);
    const dir = path.join(out, harness);
    fs.mkdirSync(dir, { recursive: true });
    const metaFile = path.join(dir, "meta.json");
    const meta = fs.existsSync(metaFile)
      ? JSON.parse(fs.readFileSync(metaFile, "utf8"))
      : { harness, scenarios: {} };
    const sanitizeVersion = makeSanitizer({ roots: [[work, "work"]] });
    meta.version = sanitizeVersion.text(version(harness));
    meta.gaps = driver.GAPS ?? {};
    const scenarios = driver.SCENARIOS.filter((name) => !only.length || only.includes(name));
    for (const scenario of scenarios) {
      const scratch = path.join(work, `${harness}-${scenario}`);
      const { repo, emptyHome } = scratchRepo(scratch);
      process.stdout.write(`${harness}/${scenario} ... `);
      const result = await driver.run(scenario, { cwd: repo, env: process.env, emptyHome, image, model });
      const sanitizer = makeSanitizer({ roots: [[repo, "repo"], [emptyHome, "empty-home"], [work, "work"]] });
      const lines = result.wire.records.map((record) => JSON.stringify(sanitizer.record(record)));
      const body = `${lines.join("\n")}\n`;
      const bytes = Buffer.byteLength(body);
      if (bytes > MAX_TRANSCRIPT_BYTES) {
        throw new Error(`${harness}/${scenario} is ${bytes} bytes, over ${MAX_TRANSCRIPT_BYTES}`);
      }
      fs.writeFileSync(path.join(dir, `${scenario}.jsonl`), body);
      meta.args = result.args.map((arg) => sanitizer.text(arg));
      meta.scenarios[scenario] = {
        outcome: sanitizer.text(result.outcome),
        firstFrameMs: result.wire.firstFrameMs,
        frames: result.wire.records.length,
        bytes,
        exit: result.wire.exit,
        stderrTail: sanitizer.text(result.wire.stderr.slice(-600)),
      };
      process.stdout.write(`${result.outcome} (${result.wire.records.length} frames, ${bytes} bytes)\n`);
    }
    fs.writeFileSync(metaFile, `${JSON.stringify(meta, null, 2)}\n`);
    fs.writeFileSync(path.join(dir, "shape.json"), `${JSON.stringify(shapeOfHarness(dir), null, 2)}\n`);
  }
}

main().catch((error) => {
  console.error(error.stack ?? String(error));
  process.exit(1);
});
