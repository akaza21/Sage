#!/usr/bin/env node
// Sage lane worktrees: one ticket, one branch, one worktree, one writer.
//
//   node scripts/lane.mjs new <ticket>            create lane/<ticket> from origin/main
//   node scripts/lane.mjs list                    show lanes
//   node scripts/lane.mjs done <ticket> [flags]  integrate, publish, then safely clean up
//   node scripts/lane.mjs drop <ticket>           remove a lane without integrating
//
// Flags for done: --gate=fast|full|none (default full), --no-push

import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  CleanupBlockedError,
  cleanupFromReceipt,
  observeCleanupStates,
  inspectWorktree,
} from "./lane-cleanup.mjs";
import { createSystemOps } from "./lane-ops.mjs";
import {
  createLaneIdentity,
  createReceipt,
  readPendingReceipt,
} from "./lane-policy.mjs";

const defaultRepo = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);

function parseArguments(argv) {
  const [command, ticket, ...rest] = argv;
  const flags = Object.fromEntries(
    rest
      .filter((argument) => argument.startsWith("--"))
      .map((argument) => {
        const [key, value] = argument.slice(2).split("=");
        return [key, value ?? true];
      }),
  );
  return { command, ticket, flags };
}

function configuration(options) {
  const platform = options.platform ?? process.platform;
  const win = platform === "win32";
  const env = options.env ?? process.env;
  const repo = options.repo ?? defaultRepo;
  return {
    repo,
    platform,
    workRoot:
      env.SAGE_WORK_DIR ??
      (win ? "C:\\SageWork" : path.join(homedir(), "SageWork")),
    cargoRoot:
      env.SAGE_CARGO_DIR ??
      (win
        ? "C:\\SageCaches\\cargo-target"
        : path.join(homedir(), ".cache", "sage-target")),
  };
}

function ensureMainClean(lane, ops) {
  if (ops.currentBranch() !== "main") {
    throw new Error("canonical checkout must be on main");
  }
  if (ops.status(lane.repo) !== "") {
    throw new Error("canonical main working tree is not clean");
  }
}

function ensureNoReceipt(lane, ops, command) {
  let pending;
  try {
    pending = readPendingReceipt(lane, ops);
  } catch {
    pending = { state: "invalid" };
  }
  if (pending.state !== "absent") {
    throw new CleanupBlockedError(
      "receipt",
      `${command} refused while a pending or invalid lane receipt exists`,
    );
  }
}

function writeCargoConfig(worktree, target) {
  mkdirSync(path.join(worktree, ".cargo"), { recursive: true });
  writeFileSync(
    path.join(worktree, ".cargo", "config.toml"),
    `[build]\ntarget-dir = '${target}'\n`,
  );
}

function validateDoneFlags(flags) {
  const gateMode = flags.gate ?? "full";
  if (!new Set(["fast", "full", "none"]).has(gateMode)) {
    throw new Error("--gate must be fast, full, or none");
  }
  const unknownFlags = Object.keys(flags).filter(
    (flag) => flag !== "gate" && flag !== "no-push",
  );
  if (
    unknownFlags.length ||
    (flags["no-push"] !== undefined && flags["no-push"] !== true)
  ) {
    throw new Error("done accepts only --gate=fast|full|none and --no-push");
  }
  return gateMode;
}

function integrationDetails(lane, flags, ops) {
  const gateMode = validateDoneFlags(flags);
  const initialCommit = ops.refTip(lane.branchRef);
  const initialWorktree = inspectWorktree(
    lane,
    { laneCommit: initialCommit },
    ops,
    true,
  );
  if (!initialCommit || initialWorktree.worktree !== "registered") {
    throw new CleanupBlockedError(
      "worktree",
      "exact clean registered lane worktree required",
    );
  }
  ops.fetchMain();
  ops.rebaseMain(lane.worktreePath);
  if (gateMode !== "none") {
    ops.run("node", ["scripts/gate.mjs", gateMode], lane.worktreePath, {
      CARGO_TARGET_DIR: lane.cargoTargetPath,
    });
  }
  const laneCommit = ops.revParse("HEAD", lane.worktreePath);
  try {
    ops.mergeBranch(lane.branch);
  } catch (error) {
    // A hook/process error can follow a completed fast-forward.
    if (ops.revParse("HEAD") !== laneCommit) throw error;
  }
  const integratedCommit = ops.revParse("HEAD");
  if (integratedCommit !== laneCommit) {
    throw new Error(
      "fast-forward integration did not land the exact lane commit",
    );
  }
  const pushRequested = !flags["no-push"];
  if (pushRequested) {
    try {
      ops.pushMain();
    } catch (error) {
      let pushSucceeded;
      try {
        pushSucceeded = ops.remoteMainTip() === integratedCommit;
      } catch {
        pushSucceeded = "unknown";
      }
      if (!pushSucceeded || pushSucceeded === "unknown") {
        error.integrationDetails = {
          gateMode,
          gateStatus: gateMode === "none" ? "not-run" : "passed",
          laneCommit,
          integratedCommit,
          pushRequested: true,
          pushSucceeded,
        };
        throw error;
      }
    }
  }
  return {
    gateMode,
    gateStatus: gateMode === "none" ? "not-run" : "passed",
    laneCommit,
    integratedCommit,
    pushRequested,
    pushSucceeded: pushRequested,
  };
}

function line(value) {
  return String(value)
    .replace(/[\r\n]+/g, " ")
    .slice(0, 240);
}

function resumeCommand(lane, receipt) {
  const gate = receipt.gate?.mode ? ` --gate=${receipt.gate.mode}` : "";
  const noPush = receipt.push?.requested === false ? " --no-push" : "";
  return `npm run lane -- done ${lane.ticket}${gate}${noPush}`;
}

function printPartial(output, lane, receipt, result) {
  const states = result.states;
  output.error("LANE PARTIAL SUCCESS");
  output.error(`commit: ${receipt.integratedCommit}`);
  output.error(
    `push: requested=${receipt.push.requested} succeeded=${receipt.push.succeeded}`,
  );
  output.error(`failed stage: ${result.failedStage}`);
  output.error(`reason: ${line(result.error?.message ?? result.error)}`);
  output.error(
    `remaining: worktree=${states.worktree} path=${states.path} remote=${states.remote} local=${states.local} cache=${states.cache} receipt=${states.receipt}`,
  );
  output.error(`resume: ${resumeCommand(lane, receipt)}`);
}

function printBlocked(output, lane, error) {
  output.error("LANE CLEANUP BLOCKED");
  output.error(`failed stage: ${error.stage ?? "authority"}`);
  output.error(`reason: ${line(error.message)}`);
  output.error(`resume: npm run lane -- done ${lane.ticket}`);
}

function postIntegrationReceipt(details) {
  return {
    integratedCommit: details.integratedCommit,
    laneCommit: details.laneCommit,
    gate: {
      mode: details.gateMode,
      status: details.gateStatus,
    },
    push: {
      requested: details.pushRequested,
      succeeded: details.pushSucceeded,
    },
  };
}

function postIntegrationStates(lane, details, ops, receipt = "absent") {
  return {
    ...observeCleanupStates(lane, postIntegrationReceipt(details), ops),
    receipt,
  };
}

function done(lane, flags, ops, output) {
  validateDoneFlags(flags);
  let pending;
  try {
    pending = readPendingReceipt(lane, ops);
  } catch (error) {
    printBlocked(output, lane, {
      stage: "receipt",
      message: error?.message ?? "receipt inspection failed",
    });
    return 1;
  }
  if (pending.state === "invalid") {
    printBlocked(output, lane, {
      stage: "receipt",
      message: pending.error.message,
    });
    return 1;
  }
  let receipt;
  if (pending.state === "valid") {
    receipt = pending.receipt;
    try {
      ensureMainClean(lane, ops);
    } catch (error) {
      printBlocked(output, lane, {
        stage: "canonical",
        message: error.message,
      });
      return 1;
    }
  } else {
    ensureMainClean(lane, ops);
    let details;
    try {
      details = integrationDetails(lane, flags, ops);
    } catch (error) {
      if (!error.integrationDetails) throw error;
      details = error.integrationDetails;
      printPartial(output, lane, postIntegrationReceipt(details), {
        failedStage: "main-push",
        error,
        states: postIntegrationStates(lane, details, ops),
      });
      return 1;
    }
    try {
      receipt = createReceipt(lane, details, ops);
    } catch (error) {
      const integrated = postIntegrationReceipt(details);
      printPartial(output, lane, integrated, {
        failedStage: "receipt-create",
        error: new Error(
          `Recovery state could not be persisted/verified: ${error.message}`,
        ),
        states: observeCleanupStates(lane, integrated, ops),
      });
      return 1;
    }
  }
  try {
    const result = cleanupFromReceipt(lane, receipt, ops);
    if (!result.ok) {
      printPartial(output, lane, receipt, result);
      return 1;
    }
  } catch (error) {
    if (error instanceof CleanupBlockedError) {
      printBlocked(output, lane, error);
      return 1;
    }
    throw error;
  }
  output.log(
    `integrated ${lane.branch} into main at ${receipt.integratedCommit}; push requested=${receipt.push.requested} succeeded=${receipt.push.succeeded}; cleanup complete`,
  );
  output.log("Integration/publication is not CI or acceptance.");
  return 0;
}

export function runLane(argv, options = {}) {
  const output = options.output ?? {
    log: console.log,
    error: console.error,
  };
  const config = configuration(options);
  const ops = options.ops ?? createSystemOps(config);
  let lane;
  try {
    const { command, ticket, flags } = parseArguments(argv);
    if (command === "list") {
      output.log(ops.listWorktreesHuman());
      return 0;
    }
    if (!new Set(["new", "done", "drop"]).has(command) || !ticket) {
      throw new Error("usage: lane.mjs new|list|done|drop <ticket>");
    }
    lane = createLaneIdentity({ ...config, ticket });
    ops.checkRefFormat(lane.branch);
    if (command === "new") {
      ensureNoReceipt(lane, ops, "new");
      ensureMainClean(lane, ops);
      ops.fetchMain();
      if (existsSync(lane.worktreePath))
        throw new Error(`${lane.worktreePath} already exists`);
      ops.addWorktree(lane.branch, lane.worktreePath);
      output.log(
        `worktree ${lane.worktreePath} on ${lane.branch} from origin/main`,
      );
      writeCargoConfig(lane.worktreePath, lane.cargoTargetPath);
      ops.run("npm", ["ci", "--no-audit", "--no-fund"], lane.worktreePath);
      output.log(`lane ready; Cargo target ${lane.cargoTargetPath}`);
      return 0;
    }
    if (command === "done") return done(lane, flags, ops, output);
    ensureNoReceipt(lane, ops, "drop");
    if (existsSync(lane.worktreePath))
      ops.forceRemoveWorktree(lane.worktreePath);
    try {
      ops.forceDeleteLocalBranch(lane.branch);
    } catch {
      // Ordinary drop continues to tolerate an already absent branch.
    }
    ops.removeDroppedCargoTarget(lane.cargoTargetPath);
    output.log(`dropped lane ${ticket}`);
    return 0;
  } catch (error) {
    if (error instanceof CleanupBlockedError && lane) {
      printBlocked(output, lane, error);
    } else {
      output.error(line(error?.message ?? error));
    }
    return 1;
  }
}

const isDirectEntry =
  process.argv[1] &&
  pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url;
if (isDirectEntry) process.exitCode = runLane(process.argv.slice(2));
