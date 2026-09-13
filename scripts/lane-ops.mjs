import { execFileSync, spawnSync } from "node:child_process";
import {
  closeSync,
  lstatSync,
  mkdirSync,
  openSync,
  readSync,
  realpathSync,
  renameSync,
  rmSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";

import { parseWorktreePorcelain } from "./lane-policy.mjs";

function isMissingRef(error) {
  return error?.status === 1 && error?.stdout === "";
}

export function createSystemOps({ repo, platform = process.platform }) {
  const win = platform === "win32";

  function git(args, cwd = repo, options = {}) {
    return execFileSync("git", args, {
      cwd,
      encoding: "utf8",
      stdio: options.quiet ? "pipe" : ["ignore", "pipe", "inherit"],
      ...options,
    }).trim();
  }

  return {
    lstat: lstatSync,
    readBounded(target, limit) {
      const descriptor = openSync(target, "r");
      try {
        const buffer = Buffer.alloc(limit);
        const bytes = readSync(descriptor, buffer, 0, limit, 0);
        return buffer.toString("utf8", 0, bytes);
      } finally {
        closeSync(descriptor);
      }
    },
    mkdir: (target) => mkdirSync(target, { recursive: true }),
    // No fsync durability claim: reader validation rejects torn writes.
    writeExclusive: (target, body) =>
      writeFileSync(target, body, { encoding: "utf8", flag: "wx" }),
    rename: renameSync,
    unlink: unlinkSync,
    realpath: realpathSync.native,
    removeCargoTarget: (target) => rmSync(target, { recursive: true }),
    currentBranch: () => git(["branch", "--show-current"]),
    status: (cwd) => git(["status", "--porcelain"], cwd),
    isAncestor: (ancestor, descendant) => {
      const result = spawnSync(
        "git",
        ["merge-base", "--is-ancestor", ancestor, descendant],
        { cwd: repo, stdio: "ignore", shell: false },
      );
      if (result.status === 0) return true;
      if (result.status === 1) return false;
      throw new Error(`git merge-base inspection failed (${result.status})`);
    },
    refTip: (ref) => {
      try {
        // --quiet gives an unambiguous exit 1 for absence (without it Git
        // reports exit 128, indistinguishable from other inspection failures).
        git(["show-ref", "--verify", "--quiet", ref], repo, { quiet: true });
        return git(["rev-parse", "--verify", ref], repo, { quiet: true });
      } catch (error) {
        if (isMissingRef(error)) return undefined;
        throw error;
      }
    },
    remoteTip: (ref) => {
      let output;
      try {
        output = git(
          ["ls-remote", "--exit-code", "--refs", "origin", ref],
          repo,
          { quiet: true },
        );
      } catch (error) {
        if (error?.status === 2 && error?.stdout === "") return undefined;
        throw error;
      }
      const lines = output.split("\n").filter(Boolean);
      if (lines.length !== 1) {
        throw new Error("remote ref inspection returned an ambiguous result");
      }
      const [tip, remoteRef, ...extra] = lines[0].split("\t");
      if (!/^[0-9a-f]{40}$/.test(tip) || remoteRef !== ref || extra.length) {
        throw new Error("remote ref inspection returned an invalid result");
      }
      return tip;
    },
    worktrees: () =>
      parseWorktreePorcelain(
        execFileSync("git", ["worktree", "list", "--porcelain", "-z"], {
          cwd: repo,
          encoding: "utf8",
          maxBuffer: 1024 * 1024,
          stdio: ["ignore", "pipe", "pipe"],
        }),
      ),
    removeWorktree: (target) =>
      git(["worktree", "remove", target], repo, { quiet: true }),
    deleteRemoteBranch: (ref, expected) =>
      git(
        [
          "push",
          "origin",
          "--delete",
          `--force-with-lease=${ref}:${expected}`,
          ref,
        ],
        repo,
        { quiet: true },
      ),
    deleteLocalBranch: (ref, expected) =>
      git(["update-ref", "-d", ref, expected], repo, { quiet: true }),
    fetchMain: () => git(["fetch", "origin", "main"]),
    rebaseMain: (cwd) => git(["rebase", "origin/main"], cwd),
    mergeBranch: (branch) => git(["merge", "--ff-only", branch]),
    pushMain: () => git(["push", "origin", "main"]),
    remoteMainTip: () => {
      const output = git(
        ["ls-remote", "--exit-code", "--refs", "origin", "refs/heads/main"],
        repo,
        { quiet: true },
      );
      const [line, ...extraLines] = output.split("\n").filter(Boolean);
      const [tip, ref, ...extraFields] = line?.split("\t") ?? [];
      if (
        extraLines.length ||
        extraFields.length ||
        !/^[0-9a-f]{40}$/.test(tip) ||
        ref !== "refs/heads/main"
      ) {
        throw new Error("remote main inspection returned an invalid result");
      }
      return tip;
    },
    revParse: (revision, cwd = repo) =>
      git(["rev-parse", "--verify", `${revision}^{commit}`], cwd, {
        quiet: true,
      }),
    checkRefFormat: (branch) =>
      git(["check-ref-format", "--branch", branch], repo, { quiet: true }),
    addWorktree: (branch, target) =>
      git(["worktree", "add", "-b", branch, target, "origin/main"]),
    forceRemoveWorktree: (target) =>
      git(["worktree", "remove", "--force", target]),
    forceDeleteLocalBranch: (branch) =>
      git(["branch", "-D", branch], repo, { quiet: true }),
    listWorktreesHuman: () => git(["worktree", "list"]),
    removeDroppedCargoTarget: (target) =>
      rmSync(target, { recursive: true, force: true }),
    run(command, args, cwd, env = {}) {
      const result = spawnSync(command, args, {
        cwd,
        stdio: "inherit",
        shell: win,
        env: { ...process.env, ...env },
      });
      if (result.status !== 0) {
        throw new Error(
          `${command} ${args.join(" ")} failed (${result.status})`,
        );
      }
    },
  };
}
