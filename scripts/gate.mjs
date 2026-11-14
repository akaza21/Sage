#!/usr/bin/env node
// Sage local validation gate. Runs natively on Windows, macOS, and Linux.
//
//   node scripts/gate.mjs static   cheap static checks only (seconds)
//   node scripts/gate.mjs fast     static + frontend tests + clippy (minutes)
//   node scripts/gate.mjs rust     three-phase Rust test sequence only
//   node scripts/gate.mjs full     fast + rust + build + bundle + budgets + visual
//   node scripts/gate.mjs changed  only what the diff from main needs (minutes)
//
// `changed` compares the working tree with its merge base on origin/main
// (override with --base=<ref>) and picks steps the way CI picks lanes; see
// changedPlan in gate-plan.mjs. Run `full` before merging.
//
// Rust tests must run in three phases: tests that spawn real child processes
// and assert bounded deadlines fail under CPU contention, so the parallel phase
// skips them and they run serially afterwards. Coverage is identical to a
// fully serial run. See docs/development/FEATURE-ROADMAP.md "Status vocabulary".
//
// Every step writes its output to work/gate-logs/<step>.log (gitignored) and
// prints one PASS/FAIL line with timing. The process exits non-zero if any
// step failed. Steps keep running after a failure so one run reports all of
// them; pass --stop-on-fail to change that.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

import { changeScope } from "./change-scope.mjs";
import { changedPlan, plans, stepConfiguration } from "./gate-plan.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const logDir = resolve(repo, "work", "gate-logs");
mkdirSync(logDir, { recursive: true });
// tauri::generate_context! panics if frontendDist (../dist) is absent, which
// is the case in a fresh worktree before the first `npm run build`. Rust steps
// only need the directory to exist; the build step fills it.
mkdirSync(resolve(repo, "dist"), { recursive: true });

const args = process.argv.slice(2);
const stopOnFail = args.includes("--stop-on-fail");
const mode = args.find((a) => !a.startsWith("--")) ?? "fast";
const known = /^--(?:stop-on-fail|base=.+)$/;
const unknown = args.filter((a) => a.startsWith("-") && !known.test(a));
if (unknown.length > 0) {
  console.error(
    `unknown option ${unknown.join(" ")}; expected --stop-on-fail or --base=<ref>`,
  );
  process.exit(2);
}
const base =
  args.find((a) => a.startsWith("--base="))?.slice("--base=".length) ??
  "origin/main";
const plan =
  mode === "changed"
    ? changedPlan(
        changeScope(changedFiles(base), (file) =>
          existsSync(resolve(repo, file)),
        ),
      )
    : plans[mode]?.map((name) => ({ name, ...stepConfiguration(name) }));
if (!plan) {
  console.error(
    `unknown mode "${mode}"; expected changed or one of ${Object.keys(plans).join(", ")}`,
  );
  process.exit(2);
}

// Tracked changes since the merge base with `base`, both sides of renames,
// plus untracked files that are not ignored. -z keeps Git from quoting
// unusual path names.
function changedFiles(base) {
  const git = (...gitArgs) => {
    const result = spawnSync("git", gitArgs, { cwd: repo, encoding: "utf8" });
    if (result.status !== 0) return undefined;
    return result.stdout;
  };
  const names = (output) => output?.split("\0").filter(Boolean);
  const mergeBase = git("merge-base", "HEAD", base)?.trim();
  if (!mergeBase) {
    console.error(`no merge base with ${base}; pass --base=<ref>`);
    process.exit(2);
  }
  const listed = [
    names(git("diff", "-z", "--name-only", "--no-renames", mergeBase)),
    names(git("ls-files", "-z", "--others", "--exclude-standard")),
  ];
  if (listed.includes(undefined)) {
    console.error("git could not list the changed files");
    process.exit(2);
  }
  return listed.flat();
}

const shell = process.platform === "win32";
let failed = 0;
const started = Date.now();
console.log(
  `gate ${mode}: ${plan.map(({ name }) => name).join(", ") || "nothing to check"}; logs in ${logDir}`,
);

for (const { name, command, env } of plan) {
  const [cmd, ...cmdArgs] = command;
  // npx and npm are .cmd shims on Windows, which only a shell starts.
  const t0 = Date.now();
  const result = spawnSync(cmd, cmdArgs, {
    cwd: repo,
    shell: shell && cmd !== "node",
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
    env,
  });
  const seconds = ((Date.now() - t0) / 1000).toFixed(0);
  const output = `${result.stdout ?? ""}\n${result.stderr ?? ""}`;
  writeFileSync(resolve(logDir, `${name}.log`), output);
  const ok = result.status === 0;
  if (!ok) failed += 1;
  console.log(
    `${name.padEnd(16)} ${ok ? "PASS" : "FAIL"}  ${seconds.padStart(5)}s${
      ok ? "" : `   (work/gate-logs/${name}.log)`
    }`,
  );
  if (!ok && stopOnFail) break;
}

const total = ((Date.now() - started) / 1000).toFixed(0);
console.log(
  `gate ${mode}: ${failed === 0 ? "PASS" : `${failed} FAILED`} in ${total}s`,
);
process.exit(failed === 0 ? 0 : 1);
