#!/usr/bin/env node
// Sage lane worktrees: one ticket, one branch, one worktree, one writer.
//
//   node scripts/lane.mjs new <ticket>            create lane/<ticket> from origin/main
//   node scripts/lane.mjs list                    show lanes
//   node scripts/lane.mjs done <ticket> [flags]   rebase, gate, fast-forward main, push, remove
//   node scripts/lane.mjs drop <ticket>           remove a lane without integrating
//
// Flags for done: --gate=fast|full|none (default full), --no-push
//
// Layout (override with SAGE_WORK_DIR / SAGE_CARGO_DIR):
//   worktree      <SAGE_WORK_DIR>/<ticket>          default C:\SageWork or ~/SageWork
//   cargo target  <SAGE_CARGO_DIR>/<ticket>         default C:\SageCaches\cargo-target
//                 (also written to <worktree>/.cargo/config.toml, gitignored)
//
// Each lane gets its own node_modules (npm ci) and Cargo target so concurrent
// writers never share build locks. `done` deletes both.

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { resolve, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const win = process.platform === "win32";
const workDir =
  process.env.SAGE_WORK_DIR ??
  (win ? "C:\\SageWork" : join(homedir(), "SageWork"));
const cargoDir =
  process.env.SAGE_CARGO_DIR ??
  (win
    ? "C:\\SageCaches\\cargo-target"
    : join(homedir(), ".cache", "sage-target"));

const [cmd, ticket, ...rest] = process.argv.slice(2);
const flags = Object.fromEntries(
  rest
    .filter((a) => a.startsWith("--"))
    .map((a) => {
      const [k, v] = a.slice(2).split("=");
      return [k, v ?? true];
    }),
);

function git(args, cwd = repo, opts = {}) {
  return execFileSync("git", args, {
    cwd,
    encoding: "utf8",
    stdio: opts.quiet ? "pipe" : ["ignore", "pipe", "inherit"],
    ...opts,
  }).trim();
}
function run(command, args, cwd, env = {}) {
  const r = spawnSync(command, args, {
    cwd,
    stdio: "inherit",
    shell: win,
    env: { ...process.env, ...env },
  });
  if (r.status !== 0)
    throw new Error(`${command} ${args.join(" ")} failed (${r.status})`);
}
function requireTicket() {
  if (!ticket || !/^[A-Za-z0-9._-]+$/.test(ticket)) {
    console.error("ticket id required, e.g. ED-05 or p09-branch-diff");
    process.exit(2);
  }
}
const branchOf = (t) => `lane/${t}`;
const pathOf = (t) => join(workDir, t);
const targetOf = (t) => join(cargoDir, t);

// Untracked per-worktree Cargo config (gitignored) so interactive builds and
// `lane done` share one target. TOML literal string: no escaping needed.
function writeCargoConfig(worktree, target) {
  mkdirSync(join(worktree, ".cargo"), { recursive: true });
  writeFileSync(
    join(worktree, ".cargo", "config.toml"),
    `[build]
target-dir = '${target}'
`,
  );
}

function ensureMainClean() {
  if (git(["branch", "--show-current"]) !== "main")
    throw new Error("canonical checkout must be on main");
  if (git(["status", "--porcelain"]) !== "")
    throw new Error("canonical main working tree is not clean");
}

switch (cmd) {
  case "new": {
    requireTicket();
    ensureMainClean();
    git(["fetch", "origin", "main"]);
    const path = pathOf(ticket);
    if (existsSync(path)) throw new Error(`${path} already exists`);
    git(["worktree", "add", "-b", branchOf(ticket), path, "origin/main"]);
    console.log(`worktree ${path} on ${branchOf(ticket)} from origin/main`);
    writeCargoConfig(path, targetOf(ticket));
    run("npm", ["ci", "--no-audit", "--no-fund"], path);
    console.log(
      `
lane ready:
  cd ${path}
  npm run gate:fast   (Cargo target ${targetOf(ticket)} via untracked .cargo/config.toml)`,
    );
    break;
  }
  case "list": {
    console.log(git(["worktree", "list"]));
    break;
  }
  case "done": {
    requireTicket();
    ensureMainClean();
    const path = pathOf(ticket);
    const branch = branchOf(ticket);
    if (!existsSync(path)) throw new Error(`no worktree at ${path}`);
    if (git(["status", "--porcelain"], path) !== "")
      throw new Error(`${path} has uncommitted changes`);
    git(["fetch", "origin", "main"]);
    git(["rebase", "origin/main"], path);
    const gate = flags.gate ?? "full";
    if (gate !== "none") {
      run("node", ["scripts/gate.mjs", gate], path, {
        CARGO_TARGET_DIR: targetOf(ticket),
      });
    }
    git(["merge", "--ff-only", branch]);
    if (!flags["no-push"]) git(["push", "origin", "main"]);
    git(["worktree", "remove", path]);
    git(["branch", "-d", branch]);
    if (!flags["no-push"]) {
      try {
        git(["push", "origin", "--delete", branch], repo, { quiet: true });
      } catch {
        /* branch was never pushed */
      }
    }
    rmSync(targetOf(ticket), { recursive: true, force: true });
    console.log(
      `integrated ${branch} into main at ${git(["rev-parse", "--short", "HEAD"])}`,
    );
    break;
  }
  case "drop": {
    requireTicket();
    const path = pathOf(ticket);
    if (existsSync(path)) git(["worktree", "remove", "--force", path]);
    try {
      git(["branch", "-D", branchOf(ticket)], repo, { quiet: true });
    } catch {
      /* no branch */
    }
    rmSync(targetOf(ticket), { recursive: true, force: true });
    console.log(`dropped lane ${ticket}`);
    break;
  }
  default:
    console.error("usage: lane.mjs new|list|done|drop <ticket>");
    process.exit(2);
}
