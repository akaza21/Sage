#!/usr/bin/env node
// Records each installed harness's wire through the fixed scenarios, sanitized,
// into <out>/<harness>/. Local tooling for the Omen; it spends real harness
// usage and is never part of a gate.
//
// Signed-in scenarios run on the harness's own config dir and sign-in, as Sage
// runs it, in a scratch repository; no sign-in file is ever copied, because a
// token refreshed inside a copy leaves the owner's real one stale
// (config-env.mjs). So a capture can list the account's own plugins, skills or
// servers: triage every added name in `shape.mjs --diff` before committing.
//
//   node scripts/harness-wire/capture.mjs --out <dir> [--harness claude,codex,pi]
//        [--scenario shell,edit] [--model <claude model>] [--work <scratch dir>]
//
// Then compare with the committed corpus:
//   node scripts/harness-wire/shape.mjs --diff src-tauri/tests/fixtures/harness-wire <dir>
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import * as claude from "./claude.mjs";
import * as codex from "./codex.mjs";
import * as pi from "./pi.mjs";
import {
  assertEmptyHome,
  configRoots,
  freshScratch,
  scenarioEnv,
} from "./config-env.mjs";
import { makeSanitizer } from "./sanitize.mjs";
import { shapeOfHarness } from "./shape.mjs";
import { MAX_TRANSCRIPT_BYTES } from "./limits.mjs";
import { twoColourPng } from "./wire.mjs";

// A frame over this size that commands.jsonl already holds verbatim (the
// session handshake every scenario repeats) is stored as a reference to it.
const DEDUPE_BYTES = 1024;
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

function dedupe(records, dir) {
  const file = path.join(dir, "commands.jsonl");
  const base = fs.existsSync(file) ? readTranscriptLines(file) : [];
  const index = new Map(base.map((line) => [key(JSON.parse(line)), line]));
  return records.map((record) => {
    const line = JSON.stringify(record);
    const stored = index.get(key(record));
    // Named by the content hash of the stored line, so a later recapture of
    // commands.jsonl alone can never re-point the reference.
    return line.length > DEDUPE_BYTES && stored !== undefined
      ? JSON.stringify({
          direction: record.direction,
          ms: record.ms,
          ref: "commands.jsonl",
          sha256: createHash("sha256")
            .update(stored)
            .digest("hex")
            .slice(0, 16),
        })
      : line;
  });
}

function key(record) {
  return JSON.stringify([record.direction, record.frame]);
}

function readTranscriptLines(file) {
  return fs
    .readFileSync(file, "utf8")
    .split("\n")
    .filter((line) => line.trim());
}

function scratchRepo(dir) {
  freshScratch(dir);
  const repo = path.join(dir, "repo");
  fs.mkdirSync(path.join(repo, "src"), { recursive: true });
  fs.writeFileSync(
    path.join(repo, "README.md"),
    "# Wire capture scratch repo\n\nA disposable repository.\n",
  );
  fs.writeFileSync(
    path.join(repo, "src", "greet.js"),
    "export function greet(name) {\n  return 'hello ' + name;\n}\n",
  );
  const git = (...args) =>
    execFileSync(
      "git",
      [
        "-c",
        "user.name=Capture",
        "-c",
        "user.email=capture@example.invalid",
        ...args,
      ],
      {
        cwd: repo,
        windowsHide: true,
      },
    );
  git("init", "-q");
  git("add", ".");
  git("commit", "-q", "-m", "initial");
  const emptyHome = path.join(dir, "empty-home");
  fs.mkdirSync(emptyHome, { recursive: true });
  return { repo, emptyHome };
}

// Scrubs an already written capture again, on the host and with the --work
// dir that recorded it, after the sanitizer learns a new spelling.
function resanitize(out, harnesses, work) {
  for (const harness of harnesses) {
    const dir = path.join(out, harness);
    for (const file of fs
      .readdirSync(dir)
      .filter((name) => name.endsWith(".jsonl"))) {
      const scratch = path.join(work, `${harness}-${file.slice(0, -6)}`);
      const sanitizer = makeSanitizer({
        roots: [
          [path.join(scratch, "repo"), "repo"],
          [path.join(scratch, "empty-home"), "empty-home"],
          ...configRoots(DRIVERS[harness].CONFIG, process.env),
          [work, "work"],
        ],
      });
      const records = readTranscriptLines(path.join(dir, file)).map((line) =>
        JSON.parse(line),
      );
      const body = sanitizer
        .records(records)
        .map((record) => JSON.stringify(record));
      fs.writeFileSync(path.join(dir, file), `${body.join("\n")}\n`);
    }
    fs.writeFileSync(
      path.join(dir, "shape.json"),
      `${JSON.stringify(shapeOfHarness(dir), null, 2)}\n`,
    );
  }
}

async function main() {
  const out = option("out");
  if (!out) throw new Error("--out <dir> is required");
  const harnesses = option("harness", "claude,codex,pi").split(",");
  const only = option("scenario", "").split(",").filter(Boolean);
  const model = option("model", undefined);
  if (process.argv.includes("--resanitize")) {
    const work = option("work");
    if (!work)
      throw new Error("--resanitize needs the --work the capture used");
    return resanitize(out, harnesses, work);
  }
  // A new scratch dir for every run: the capture never clears an old one.
  const work =
    option("work") ??
    fs.mkdtempSync(path.join(os.tmpdir(), "sage-wire-capture-"));
  const image = await twoColourPng();
  let failures = 0;
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
    const scenarios = driver.SCENARIOS.filter(
      (name) => !only.length || only.includes(name),
    );
    for (const scenario of scenarios) {
      const scratch = path.join(work, `${harness}-${scenario}`);
      const { repo, emptyHome } = scratchRepo(scratch);
      assertEmptyHome(emptyHome);
      process.stdout.write(`${harness}/${scenario} ... `);
      const result = await driver.run(scenario, {
        cwd: repo,
        env: scenarioEnv(driver.CONFIG, scenario, process.env, emptyHome),
        image,
        model,
      });
      const sanitizer = makeSanitizer({
        roots: [
          [repo, "repo"],
          [emptyHome, "empty-home"],
          ...configRoots(driver.CONFIG, process.env),
          [work, "work"],
        ],
      });
      if (result.gap) {
        meta.gaps[scenario] = sanitizer.text(result.gap);
        delete meta.scenarios[scenario];
        fs.rmSync(path.join(dir, `${scenario}.jsonl`), { force: true });
        process.stdout.write(`gap: ${result.gap}\n`);
        continue;
      }
      const records = sanitizer.records(result.wire.records);
      const lines =
        scenario === "commands"
          ? records.map((r) => JSON.stringify(r))
          : dedupe(records, dir);
      const body = `${lines.join("\n")}\n`;
      const bytes = Buffer.byteLength(body);
      if (bytes > MAX_TRANSCRIPT_BYTES) {
        result.outcome = `oversize: ${bytes} bytes, not written`;
        failures += 1;
      } else {
        fs.writeFileSync(path.join(dir, `${scenario}.jsonl`), body);
      }
      meta.args = result.args.map((arg) => sanitizer.text(arg));
      meta.scenarios[scenario] = {
        outcome: sanitizer.text(result.outcome),
        firstFrameMs: result.wire.firstFrameMs,
        frames: result.wire.records.length,
        bytes,
        exit: result.wire.exit,
        stderrTail: sanitizer.text(result.wire.stderr.slice(-600)),
      };
      process.stdout.write(
        `${result.outcome} (${result.wire.records.length} frames, ${bytes} bytes)\n`,
      );
    }
    fs.writeFileSync(metaFile, `${JSON.stringify(meta, null, 2)}\n`);
    fs.writeFileSync(
      path.join(dir, "shape.json"),
      `${JSON.stringify(shapeOfHarness(dir), null, 2)}\n`,
    );
  }
  if (failures) throw new Error(`${failures} scenarios were not written`);
}

main().catch((error) => {
  console.error(error.stack ?? String(error));
  process.exit(1);
});
