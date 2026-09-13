import path from "node:path";

import {
  readPendingReceipt,
  receiptPresence,
  samePath,
} from "./lane-policy.mjs";

export class CleanupBlockedError extends Error {
  constructor(stage, message) {
    super(message);
    this.name = "CleanupBlockedError";
    this.stage = stage;
  }
}

function blocked(stage, message) {
  throw new CleanupBlockedError(stage, message);
}

function relevantWorktrees(lane, records) {
  return records.filter(
    (record) =>
      samePath(record.path, lane.worktreePath, lane.platform) ||
      record.branch === lane.branchRef,
  );
}

function inspectPath(ops, target) {
  try {
    const stat = ops.lstat(target);
    return {
      state: stat.isSymbolicLink()
        ? "link"
        : stat.isDirectory()
          ? "directory"
          : "other",
      stat,
    };
  } catch (error) {
    if (error?.code === "ENOENT") return { state: "absent" };
    throw error;
  }
}

function physicallyContained(ops, root, target, platform) {
  const physicalRoot = ops.realpath(root);
  const physicalTarget = ops.realpath(target);
  const relative = path.relative(physicalRoot, physicalTarget);
  const fold = (value) => (platform === "win32" ? value.toLowerCase() : value);
  return (
    relative !== "" &&
    relative !== ".." &&
    !relative.startsWith(`..${path.sep}`) &&
    !path.isAbsolute(relative) &&
    fold(path.resolve(physicalRoot, relative)) ===
      fold(path.resolve(physicalTarget))
  );
}

export function inspectWorktree(lane, receipt, ops, requireClean = false) {
  const records = relevantWorktrees(lane, ops.worktrees());
  const pathState = inspectPath(ops, lane.worktreePath);
  if (records.length === 0) {
    return {
      worktree: "absent",
      path: pathState.state === "absent" ? "absent" : "residual",
    };
  }
  if (records.length !== 1) {
    return { worktree: "mismatch", path: pathState.state };
  }
  const record = records[0];
  if (
    !samePath(record.path, lane.worktreePath, lane.platform) ||
    record.branch !== lane.branchRef ||
    record.head !== receipt.laneCommit
  ) {
    return { worktree: "mismatch", path: pathState.state };
  }
  if (pathState.state !== "directory") {
    return { worktree: "mismatch", path: pathState.state };
  }
  if (requireClean && ops.status(lane.worktreePath) !== "") {
    return { worktree: "dirty", path: "present" };
  }
  return { worktree: "registered", path: "present" };
}

export function inspectLocalBranch(lane, receipt, ops) {
  const tip = ops.refTip(lane.branchRef);
  if (tip === undefined) return "absent";
  return tip === receipt.laneCommit ? "matching" : "mismatch";
}

export function inspectRemoteBranch(lane, receipt, ops) {
  if (!receipt.push.requested) return "not-requested";
  const tip = ops.remoteTip(lane.remoteBranchRef);
  if (tip === undefined) return "absent";
  return tip === receipt.laneCommit ? "matching" : "mismatch";
}

export function inspectCargoTarget(lane, ops) {
  const target = inspectPath(ops, lane.cargoTargetPath);
  if (target.state === "absent") return "absent";
  if (target.state !== "directory") return "unsafe";
  try {
    return physicallyContained(
      ops,
      lane.cargoRoot,
      lane.cargoTargetPath,
      lane.platform,
    )
      ? "present"
      : "unsafe";
  } catch {
    return "unsafe";
  }
}

export function verifyCleanupAuthority(lane, receipt, ops) {
  try {
    if (ops.currentBranch() !== "main") {
      blocked("canonical", "canonical checkout must be on main");
    }
    if (ops.status(lane.repo) !== "") {
      blocked("canonical", "canonical main working tree is not clean");
    }
    if (!ops.isAncestor(receipt.integratedCommit, "main")) {
      blocked(
        "ancestry",
        "recorded integrated commit is not an ancestor of main",
      );
    }
    const local = inspectLocalBranch(lane, receipt, ops);
    if (local === "mismatch") {
      blocked("local-branch", "surviving local lane branch changed tip");
    }
    const worktree = inspectWorktree(lane, receipt, ops, true);
    if (worktree.worktree === "mismatch") {
      blocked(
        "worktree",
        "registered lane worktree does not have the exact path, branch, and HEAD",
      );
    }
    if (worktree.worktree === "dirty") {
      blocked("worktree", "registered lane worktree is not clean");
    }
    if (worktree.path === "residual") {
      blocked(
        "worktree-path",
        "unregistered residual worktree path exists; refusing raw deletion or prune",
      );
    }
    if (worktree.worktree === "registered" && local !== "matching") {
      blocked(
        "local-branch",
        "registered worktree has no matching local branch",
      );
    }
    const cargo = inspectCargoTarget(lane, ops);
    if (cargo === "unsafe") {
      blocked(
        "cargo-cache",
        "Cargo target is not an exact contained non-link directory",
      );
    }
    let remote;
    try {
      remote = inspectRemoteBranch(lane, receipt, ops);
    } catch {
      blocked(
        "remote-branch",
        "remote lane ref state is unknown due to transport or authentication failure",
      );
    }
    if (remote === "mismatch") {
      blocked("remote-branch", "remote lane ref changed tip");
    }
    return { worktree, local, remote, cargo };
  } catch (error) {
    if (error instanceof CleanupBlockedError) throw error;
    blocked(
      "authority",
      error?.message ?? "cleanup authority inspection failed",
    );
  }
}

function safeInspect(inspect, unknown = "unknown") {
  try {
    return inspect();
  } catch {
    return unknown;
  }
}

export function observeCleanupStates(lane, receipt, ops) {
  const worktree = safeInspect(() => inspectWorktree(lane, receipt, ops));
  const receiptState = safeInspect(() => {
    const pending = readPendingReceipt(lane, ops);
    return pending.state;
  });
  return {
    worktree: typeof worktree === "string" ? worktree : worktree.worktree,
    path: typeof worktree === "string" ? "unknown" : worktree.path,
    remote: safeInspect(() => inspectRemoteBranch(lane, receipt, ops)),
    local: safeInspect(() => inspectLocalBranch(lane, receipt, ops)),
    cache: safeInspect(() => inspectCargoTarget(lane, ops)),
    receipt: receiptState,
  };
}

function partial(stage, error, lane, receipt, ops) {
  return {
    ok: false,
    failedStage: stage,
    error,
    states: observeCleanupStates(lane, receipt, ops),
  };
}

function worktreeConverged(lane, receipt, ops) {
  const state = inspectWorktree(lane, receipt, ops);
  return state.worktree === "absent" && state.path === "absent";
}

function remoteConverged(lane, receipt, ops) {
  return inspectRemoteBranch(lane, receipt, ops) === "absent";
}

function localConverged(lane, receipt, ops) {
  return inspectLocalBranch(lane, receipt, ops) === "absent";
}

function cacheConverged(lane, ops) {
  return inspectCargoTarget(lane, ops) === "absent";
}

function performAndReinspect(stage, perform, converged, lane, receipt, ops) {
  let operationError;
  try {
    perform();
  } catch (error) {
    operationError = error;
  }
  try {
    if (converged()) return undefined;
  } catch (error) {
    return partial(stage, operationError ?? error, lane, receipt, ops);
  }
  return partial(
    stage,
    operationError ?? new Error(`${stage} did not converge`),
    lane,
    receipt,
    ops,
  );
}

function sameReceipt(expected, actual) {
  return JSON.stringify(expected) === JSON.stringify(actual);
}

export function cleanupFromReceipt(lane, receipt, ops) {
  const authority = verifyCleanupAuthority(lane, receipt, ops);

  if (authority.worktree.worktree === "registered") {
    const failed = performAndReinspect(
      "worktree",
      () => ops.removeWorktree(lane.worktreePath),
      () => worktreeConverged(lane, receipt, ops),
      lane,
      receipt,
      ops,
    );
    if (failed) return failed;
  }

  let remote;
  try {
    remote = inspectRemoteBranch(lane, receipt, ops);
    if (remote === "mismatch") throw new Error("remote lane ref changed tip");
  } catch (error) {
    return partial("remote-branch", error, lane, receipt, ops);
  }
  if (remote === "matching") {
    const failed = performAndReinspect(
      "remote-branch",
      () => ops.deleteRemoteBranch(lane.remoteBranchRef, receipt.laneCommit),
      () => remoteConverged(lane, receipt, ops),
      lane,
      receipt,
      ops,
    );
    if (failed) return failed;
  }

  if (authority.local === "matching") {
    const failed = performAndReinspect(
      "local-branch",
      () => ops.deleteLocalBranch(lane.branchRef, receipt.laneCommit),
      () => localConverged(lane, receipt, ops),
      lane,
      receipt,
      ops,
    );
    if (failed) return failed;
  }

  let cargo;
  try {
    cargo = inspectCargoTarget(lane, ops);
    if (cargo === "unsafe")
      throw new Error("Cargo target changed to an unsafe path");
  } catch (error) {
    return partial("cargo-cache", error, lane, receipt, ops);
  }
  if (cargo === "present") {
    const failed = performAndReinspect(
      "cargo-cache",
      () => ops.removeCargoTarget(lane.cargoTargetPath),
      () => cacheConverged(lane, ops),
      lane,
      receipt,
      ops,
    );
    if (failed) return failed;
  }

  try {
    const finalAuthority = verifyCleanupAuthority(lane, receipt, ops);
    if (
      finalAuthority.worktree.worktree !== "absent" ||
      finalAuthority.worktree.path !== "absent" ||
      finalAuthority.remote === "matching" ||
      finalAuthority.local !== "absent" ||
      finalAuthority.cargo !== "absent"
    ) {
      return partial(
        "final-verification",
        new Error("cleanup resources remain"),
        lane,
        receipt,
        ops,
      );
    }
    const pending = readPendingReceipt(lane, ops);
    if (pending.state !== "valid" || !sameReceipt(receipt, pending.receipt)) {
      return partial(
        "receipt",
        new Error("receipt changed before removal"),
        lane,
        receipt,
        ops,
      );
    }
  } catch (error) {
    return partial("final-verification", error, lane, receipt, ops);
  }

  const failed = performAndReinspect(
    "receipt",
    () => ops.unlink(lane.receiptPath),
    () => receiptPresence(lane, ops) === "absent",
    lane,
    receipt,
    ops,
  );
  if (failed) return failed;
  return { ok: true, states: observeCleanupStates(lane, receipt, ops) };
}
