import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import {
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  renameSync,
  rmSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { createSystemOps } from "./lane-ops.mjs";
import {
  createLaneIdentity,
  createReceipt,
  parseWorktreePorcelain,
  readPendingReceipt,
  validateTicket,
} from "./lane-policy.mjs";
import { runLane } from "./lane.mjs";

const commit = "a".repeat(40);
const otherCommit = "b".repeat(40);

function removeDirectory(target) {
  rmSync(target, { recursive: true, force: true });
}

function makeFixture(options = {}) {
  const root = mkdtempSync(path.join(tmpdir(), "sage-lane-test-"));
  const repo = path.join(root, "repo");
  const workRoot = path.join(root, "worktrees");
  const cargoRoot = path.join(root, "cargo");
  const ticket = options.ticket ?? "LANE-TEST";
  mkdirSync(repo, { recursive: true });
  mkdirSync(workRoot, { recursive: true });
  mkdirSync(cargoRoot, { recursive: true });
  const lane = createLaneIdentity({
    repo,
    workRoot,
    cargoRoot,
    ticket,
    platform: process.platform,
  });
  mkdirSync(lane.worktreePath, { recursive: true });
  mkdirSync(lane.cargoTargetPath, { recursive: true });

  const state = {
    currentBranch: "main",
    canonicalStatus: "",
    laneStatus: "",
    ancestor: true,
    localTip: commit,
    remoteTip: options.remoteTip === undefined ? commit : options.remoteTip,
    remoteError: undefined,
    mainPushError: undefined,
    remoteMainTip: commit,
    remoteMainError: undefined,
    renameError: undefined,
    worktree: {
      registered: true,
      path: lane.worktreePath,
      branch: lane.branchRef,
      head: commit,
    },
    failWorktree: undefined,
    failRemote: undefined,
    failLocal: undefined,
    failCache: undefined,
    lastReceipt: undefined,
    remoteInspections: 0,
  };
  const actions = [];
  const logs = [];
  const errors = [];

  function failAt(mode, mutate) {
    if (mode === "before") throw new Error("injected failure");
    mutate();
    if (mode === "after") throw new Error("injected converged failure");
  }

  const ops = {
    lstat: lstatSync,
    readBounded: (target, limit) =>
      readFileSync(target).subarray(0, limit).toString("utf8"),
    mkdir: (target) => mkdirSync(target, { recursive: true }),
    writeExclusive(target, body) {
      actions.push("receipt-write");
      writeFileSync(target, body, { encoding: "utf8", flag: "wx" });
    },
    rename(from, to) {
      actions.push("receipt-rename");
      if (state.renameError) throw state.renameError;
      renameSync(from, to);
      state.lastReceipt = JSON.parse(readFileSync(to, "utf8"));
    },
    unlink(target) {
      actions.push(
        target === lane.receiptPath ? "receipt-remove" : "temporary-remove",
      );
      unlinkSync(target);
    },
    realpath: realpathSync.native,
    currentBranch: () => state.currentBranch,
    status: (cwd) =>
      path.resolve(cwd) === path.resolve(repo)
        ? state.canonicalStatus
        : state.laneStatus,
    isAncestor: () => state.ancestor,
    refTip: () => state.localTip,
    remoteTip() {
      state.remoteInspections += 1;
      if (state.remoteError) throw state.remoteError;
      return state.remoteTip;
    },
    worktrees: () =>
      state.worktree.registered
        ? [
            {
              path: state.worktree.path,
              branch: state.worktree.branch,
              head: state.worktree.head,
            },
          ]
        : [],
    removeWorktree(target) {
      actions.push("worktree-remove");
      if (state.failWorktree === "before") {
        throw Object.assign(new Error("EPERM: permission denied"), {
          code: "EPERM",
        });
      }
      failAt(state.failWorktree, () => {
        assert.equal(target, lane.worktreePath);
        state.worktree.registered = false;
        removeDirectory(lane.worktreePath);
      });
    },
    deleteRemoteBranch(ref, expected) {
      actions.push("remote-delete");
      assert.equal(ref, lane.remoteBranchRef);
      assert.equal(expected, commit);
      failAt(state.failRemote, () => {
        state.remoteTip = undefined;
      });
    },
    deleteLocalBranch(ref, expected) {
      actions.push("local-delete");
      assert.equal(ref, lane.branchRef);
      assert.equal(expected, commit);
      failAt(state.failLocal, () => {
        state.localTip = undefined;
      });
    },
    removeCargoTarget(target) {
      actions.push("cache-remove");
      assert.equal(target, lane.cargoTargetPath);
      failAt(state.failCache, () => removeDirectory(lane.cargoTargetPath));
    },
    fetchMain() {
      actions.push("fetch");
    },
    rebaseMain(target) {
      assert.equal(target, lane.worktreePath);
      actions.push("rebase");
    },
    run(command, args, cwd, env) {
      assert.equal(command, "node");
      assert.deepEqual(args.slice(0, 1), ["scripts/gate.mjs"]);
      assert.equal(cwd, lane.worktreePath);
      assert.equal(env.CARGO_TARGET_DIR, lane.cargoTargetPath);
      actions.push(`gate-${args[1]}`);
    },
    revParse(revision, cwd = repo) {
      assert.equal(revision, "HEAD");
      return path.resolve(cwd) === path.resolve(lane.worktreePath)
        ? commit
        : commit;
    },
    mergeBranch(branch) {
      assert.equal(branch, lane.branch);
      actions.push("merge");
    },
    pushMain() {
      actions.push("main-push");
      if (state.mainPushError) throw state.mainPushError;
    },
    remoteMainTip() {
      actions.push("main-remote-inspect");
      if (state.remoteMainError) throw state.remoteMainError;
      return state.remoteMainTip;
    },
    checkRefFormat(branch) {
      assert.equal(branch, lane.branch);
    },
    listWorktreesHuman: () => "fixture worktrees",
    addWorktree() {
      actions.push("worktree-add");
    },
    forceRemoveWorktree() {
      actions.push("worktree-force-remove");
    },
    forceDeleteLocalBranch() {
      actions.push("local-force-delete");
    },
    removeDroppedCargoTarget() {
      actions.push("cache-force-remove");
    },
  };
  const output = {
    log: (...parts) => logs.push(parts.join(" ")),
    error: (...parts) => errors.push(parts.join(" ")),
  };
  const run = (args) =>
    runLane(args, {
      repo,
      platform: process.platform,
      env: { SAGE_WORK_DIR: workRoot, SAGE_CARGO_DIR: cargoRoot },
      ops,
      output,
    });
  const dispose = () => removeDirectory(root);
  return {
    root,
    repo,
    workRoot,
    cargoRoot,
    ticket,
    lane,
    state,
    actions,
    logs,
    errors,
    ops,
    output,
    run,
    dispose,
  };
}

function receiptDetails(pushRequested = true) {
  return {
    gateMode: "fast",
    gateStatus: "passed",
    laneCommit: commit,
    integratedCommit: commit,
    pushRequested,
    pushSucceeded: pushRequested,
  };
}

function installReceipt(fixture, pushRequested = true) {
  const receipt = createReceipt(
    fixture.lane,
    receiptDetails(pushRequested),
    fixture.ops,
  );
  fixture.actions.length = 0;
  return receipt;
}

function receiptJson(fixture, overrides = {}) {
  const value = {
    version: 1,
    kind: "lane-cleanup",
    ticket: fixture.ticket,
    branch: fixture.lane.branch,
    integratedCommit: commit,
    laneCommit: commit,
    worktreePath: fixture.lane.worktreePath,
    cargoTargetPath: fixture.lane.cargoTargetPath,
    gate: { mode: "fast", status: "passed" },
    push: { requested: true, succeeded: true },
    ...overrides,
  };
  return `${JSON.stringify(value, null, 2)}\n`;
}

function writeRawReceipt(fixture, body) {
  mkdirSync(fixture.lane.receiptRoot, { recursive: true });
  writeFileSync(fixture.lane.receiptPath, body, "utf8");
}

function destructive(actions) {
  return actions.filter((action) => /remove|delete|force/.test(action));
}

test("pushed done records publication before ordered teardown", () => {
  const fixture = makeFixture();
  try {
    assert.equal(fixture.run(["done", fixture.ticket, "--gate=fast"]), 0);
    assert.deepEqual(fixture.actions, [
      "fetch",
      "rebase",
      "gate-fast",
      "merge",
      "main-push",
      "receipt-write",
      "receipt-rename",
      "worktree-remove",
      "remote-delete",
      "local-delete",
      "cache-remove",
      "receipt-remove",
    ]);
    assert.deepEqual(fixture.state.lastReceipt.push, {
      requested: true,
      succeeded: true,
    });
    assert.equal(fixture.state.lastReceipt.gate.status, "passed");
    assert.equal(existsSync(fixture.lane.receiptPath), false);
    assert.match(fixture.logs.at(-1), /not CI or acceptance/);
  } finally {
    fixture.dispose();
  }
});

test("--no-push records local-only truth and never touches the remote", () => {
  const fixture = makeFixture();
  try {
    assert.equal(
      fixture.run(["done", fixture.ticket, "--gate=none", "--no-push"]),
      0,
    );
    assert.deepEqual(fixture.actions, [
      "fetch",
      "rebase",
      "merge",
      "receipt-write",
      "receipt-rename",
      "worktree-remove",
      "local-delete",
      "cache-remove",
      "receipt-remove",
    ]);
    assert.deepEqual(fixture.state.lastReceipt.push, {
      requested: false,
      succeeded: false,
    });
    assert.deepEqual(fixture.state.lastReceipt.gate, {
      mode: "none",
      status: "not-run",
    });
    assert.equal(fixture.state.remoteInspections, 0);
  } finally {
    fixture.dispose();
  }
});

test("push success followed by worktree EPERM is resumable without reintegration", () => {
  const fixture = makeFixture();
  try {
    fixture.state.failWorktree = "before";
    assert.equal(fixture.run(["done", fixture.ticket, "--gate=fast"]), 1);
    assert.match(fixture.errors.join("\n"), /LANE PARTIAL SUCCESS/);
    assert.match(fixture.errors.join("\n"), new RegExp(`commit: ${commit}`));
    assert.match(
      fixture.errors.join("\n"),
      /push: requested=true succeeded=true/,
    );
    assert.match(fixture.errors.join("\n"), /failed stage: worktree/);
    assert.match(
      fixture.errors.join("\n"),
      new RegExp(`resume: npm run lane -- done ${fixture.ticket}`),
    );
    assert.equal(readPendingReceipt(fixture.lane, fixture.ops).state, "valid");
    assert.deepEqual(fixture.actions.slice(-1), ["worktree-remove"]);

    fixture.actions.length = 0;
    fixture.errors.length = 0;
    fixture.state.failWorktree = undefined;
    assert.equal(fixture.run(["done", fixture.ticket]), 0);
    assert.deepEqual(fixture.actions, [
      "worktree-remove",
      "remote-delete",
      "local-delete",
      "cache-remove",
      "receipt-remove",
    ]);
    assert.equal(
      fixture.actions.some((action) =>
        /^(fetch|rebase|gate|merge|main-push)$/.test(action),
      ),
      false,
    );
  } finally {
    fixture.dispose();
  }
});

test("push error with exact remote-main convergence remains truthful and cleans", () => {
  const fixture = makeFixture();
  try {
    fixture.state.mainPushError = new Error("connection closed after update");
    assert.equal(fixture.run(["done", fixture.ticket, "--gate=fast"]), 0);
    assert.deepEqual(fixture.actions.slice(0, 7), [
      "fetch",
      "rebase",
      "gate-fast",
      "merge",
      "main-push",
      "main-remote-inspect",
      "receipt-write",
    ]);
    assert.equal(fixture.state.lastReceipt.push.succeeded, true);
  } finally {
    fixture.dispose();
  }
});

test("push error without convergence reports partial truth and does no teardown", () => {
  const fixture = makeFixture();
  try {
    fixture.state.mainPushError = new Error("push rejected");
    fixture.state.remoteMainTip = otherCommit;
    assert.equal(fixture.run(["done", fixture.ticket, "--gate=fast"]), 1);
    assert.match(fixture.errors.join("\n"), /LANE PARTIAL SUCCESS/);
    assert.match(fixture.errors.join("\n"), /failed stage: main-push/);
    assert.match(
      fixture.errors.join("\n"),
      /push: requested=true succeeded=false/,
    );
    assert.deepEqual(fixture.actions, [
      "fetch",
      "rebase",
      "gate-fast",
      "merge",
      "main-push",
      "main-remote-inspect",
    ]);
    assert.equal(existsSync(fixture.lane.receiptPath), false);
    assert.equal(existsSync(fixture.lane.worktreePath), true);
  } finally {
    fixture.dispose();
  }
});

test("unknown push result reports unknown and retains every cleanup resource", () => {
  const fixture = makeFixture();
  try {
    fixture.state.mainPushError = new Error("push transport failed");
    fixture.state.remoteMainError = new Error("remote unavailable");
    assert.equal(fixture.run(["done", fixture.ticket, "--gate=fast"]), 1);
    assert.match(
      fixture.errors.join("\n"),
      /push: requested=true succeeded=unknown/,
    );
    assert.equal(existsSync(fixture.lane.worktreePath), true);
    assert.equal(existsSync(fixture.lane.cargoTargetPath), true);
  } finally {
    fixture.dispose();
  }
});

test("receipt rename failure reports retained resources and leaves no final receipt", () => {
  const fixture = makeFixture();
  try {
    fixture.state.renameError = new Error("rename denied");
    assert.equal(
      fixture.run(["done", fixture.ticket, "--gate=none", "--no-push"]),
      1,
    );
    assert.match(fixture.errors.join("\n"), /LANE PARTIAL SUCCESS/);
    assert.match(fixture.errors.join("\n"), /failed stage: receipt-create/);
    assert.match(
      fixture.errors.join("\n"),
      /push: requested=false succeeded=false/,
    );
    assert.match(
      fixture.errors.join("\n"),
      new RegExp(
        `resume: npm run lane -- done ${fixture.ticket} --gate=none --no-push`,
      ),
    );
    assert.deepEqual(fixture.actions, [
      "fetch",
      "rebase",
      "merge",
      "receipt-write",
      "receipt-rename",
      "temporary-remove",
    ]);
    assert.equal(existsSync(fixture.lane.receiptPath), false);
    assert.equal(existsSync(fixture.lane.worktreePath), true);
  } finally {
    fixture.dispose();
  }
});

test("an operation error after actual convergence continues cleanup", () => {
  const fixture = makeFixture();
  try {
    installReceipt(fixture);
    fixture.state.failWorktree = "after";
    assert.equal(fixture.run(["done", fixture.ticket]), 0);
    assert.deepEqual(fixture.actions, [
      "worktree-remove",
      "remote-delete",
      "local-delete",
      "cache-remove",
      "receipt-remove",
    ]);
  } finally {
    fixture.dispose();
  }
});

test("unregistered residual worktree path blocks all teardown", () => {
  const fixture = makeFixture();
  try {
    installReceipt(fixture);
    fixture.state.worktree.registered = false;
    assert.equal(fixture.run(["done", fixture.ticket]), 1);
    assert.match(fixture.errors.join("\n"), /LANE CLEANUP BLOCKED/);
    assert.match(fixture.errors.join("\n"), /unregistered residual/);
    assert.deepEqual(destructive(fixture.actions), []);
    assert.equal(existsSync(fixture.lane.worktreePath), true);
  } finally {
    fixture.dispose();
  }
});

test("local branch failure after worktree removal retains receipt for resume", () => {
  const fixture = makeFixture({ remoteTip: null });
  try {
    fixture.state.remoteTip = undefined;
    installReceipt(fixture);
    fixture.state.failLocal = "before";
    assert.equal(fixture.run(["done", fixture.ticket]), 1);
    assert.deepEqual(fixture.actions, ["worktree-remove", "local-delete"]);
    assert.match(fixture.errors.join("\n"), /failed stage: local-branch/);
    assert.equal(existsSync(fixture.lane.worktreePath), false);
    assert.equal(existsSync(fixture.lane.receiptPath), true);
    assert.equal(existsSync(fixture.lane.cargoTargetPath), true);

    fixture.actions.length = 0;
    fixture.errors.length = 0;
    fixture.state.failLocal = undefined;
    assert.equal(fixture.run(["done", fixture.ticket]), 0);
    assert.deepEqual(fixture.actions, [
      "local-delete",
      "cache-remove",
      "receipt-remove",
    ]);
  } finally {
    fixture.dispose();
  }
});

test("Cargo cache failure occurs only after worktree and branch convergence", () => {
  const fixture = makeFixture({ remoteTip: null });
  try {
    fixture.state.remoteTip = undefined;
    installReceipt(fixture);
    fixture.state.failCache = "before";
    assert.equal(fixture.run(["done", fixture.ticket]), 1);
    assert.deepEqual(fixture.actions, [
      "worktree-remove",
      "local-delete",
      "cache-remove",
    ]);
    assert.match(fixture.errors.join("\n"), /failed stage: cargo-cache/);
    assert.equal(fixture.state.localTip, undefined);
    assert.equal(existsSync(fixture.lane.receiptPath), true);
  } finally {
    fixture.dispose();
  }
});

test("an absent pushed lane ref converges without remote deletion", () => {
  const fixture = makeFixture({ remoteTip: null });
  try {
    fixture.state.remoteTip = undefined;
    installReceipt(fixture);
    assert.equal(fixture.run(["done", fixture.ticket]), 0);
    assert.deepEqual(fixture.actions, [
      "worktree-remove",
      "local-delete",
      "cache-remove",
      "receipt-remove",
    ]);
    assert.ok(fixture.state.remoteInspections >= 1);
  } finally {
    fixture.dispose();
  }
});

for (const scenario of ["mismatch", "network-unknown"]) {
  test(`remote ${scenario} blocks before any destructive operation`, () => {
    const fixture = makeFixture();
    try {
      installReceipt(fixture);
      if (scenario === "mismatch") fixture.state.remoteTip = otherCommit;
      else fixture.state.remoteError = new Error("network unavailable");
      assert.equal(fixture.run(["done", fixture.ticket]), 1);
      assert.match(fixture.errors.join("\n"), /LANE CLEANUP BLOCKED/);
      assert.match(fixture.errors.join("\n"), /remote lane ref/);
      assert.deepEqual(destructive(fixture.actions), []);
      assert.equal(existsSync(fixture.lane.worktreePath), true);
    } finally {
      fixture.dispose();
    }
  });
}

test("malformed, mismatched, and path-unsafe receipts block without operations", async (t) => {
  const cases = [
    ["malformed", "{not json"],
    ["ticket mismatch", (f) => receiptJson(f, { ticket: "OTHER" })],
    ["commit mismatch", (f) => receiptJson(f, { laneCommit: otherCommit })],
    [
      "unsafe worktree path",
      (f) => receiptJson(f, { worktreePath: path.join(f.root, "outside") }),
    ],
    [
      "unsafe Cargo path",
      (f) => receiptJson(f, { cargoTargetPath: path.join(f.root, "outside") }),
    ],
    ["extra key", (f) => receiptJson(f, { unexpected: true })],
  ];
  for (const [name, body] of cases) {
    await t.test(name, () => {
      const fixture = makeFixture();
      try {
        writeRawReceipt(
          fixture,
          typeof body === "function" ? body(fixture) : body,
        );
        assert.equal(fixture.run(["done", fixture.ticket]), 1);
        assert.match(fixture.errors.join("\n"), /LANE CLEANUP BLOCKED/);
        assert.match(fixture.errors.join("\n"), /failed stage: receipt/);
        assert.deepEqual(fixture.actions, []);
      } finally {
        fixture.dispose();
      }
    });
  }
});

test("changed authority blocks before teardown", async (t) => {
  const cases = [
    ["canonical wrong branch", (f) => (f.state.currentBranch = "topic")],
    ["canonical dirty", (f) => (f.state.canonicalStatus = " M README.md")],
    ["nonancestor", (f) => (f.state.ancestor = false)],
    ["changed local branch", (f) => (f.state.localTip = otherCommit)],
    ["dirty worktree", (f) => (f.state.laneStatus = "?? file")],
    [
      "wrong worktree branch",
      (f) => (f.state.worktree.branch = "refs/heads/lane/OTHER"),
    ],
    ["wrong worktree HEAD", (f) => (f.state.worktree.head = otherCommit)],
    [
      "unsafe Cargo target",
      (f) => {
        removeDirectory(f.lane.cargoTargetPath);
        writeFileSync(f.lane.cargoTargetPath, "not a directory", "utf8");
      },
    ],
  ];
  for (const [name, mutate] of cases) {
    await t.test(name, () => {
      const fixture = makeFixture();
      try {
        installReceipt(fixture);
        mutate(fixture);
        assert.equal(fixture.run(["done", fixture.ticket]), 1);
        assert.match(fixture.errors.join("\n"), /LANE CLEANUP BLOCKED/);
        assert.deepEqual(destructive(fixture.actions), []);
      } finally {
        fixture.dispose();
      }
    });
  }
});

test("already absent resources converge by removing only the receipt", () => {
  const fixture = makeFixture({ remoteTip: null });
  try {
    fixture.state.remoteTip = undefined;
    installReceipt(fixture);
    fixture.state.worktree.registered = false;
    fixture.state.localTip = undefined;
    removeDirectory(fixture.lane.worktreePath);
    removeDirectory(fixture.lane.cargoTargetPath);
    assert.equal(fixture.run(["done", fixture.ticket]), 0);
    assert.deepEqual(fixture.actions, ["receipt-remove"]);
  } finally {
    fixture.dispose();
  }
});

test("pending receipt blocks new and drop", () => {
  for (const command of ["new", "drop"]) {
    const fixture = makeFixture();
    try {
      installReceipt(fixture);
      assert.equal(fixture.run([command, fixture.ticket]), 1);
      assert.match(fixture.errors.join("\n"), /LANE CLEANUP BLOCKED/);
      assert.match(
        fixture.errors.join("\n"),
        /pending or invalid lane receipt/,
      );
      assert.deepEqual(fixture.actions, []);
      assert.equal(existsSync(fixture.lane.worktreePath), true);
    } finally {
      fixture.dispose();
    }
  }
});

test("ticket admission is bounded and portable before Git ref validation", () => {
  for (const invalid of [
    ".",
    "..",
    "CON",
    "con.txt",
    "LPT9.log",
    "-leading",
    "trailing.",
    "has/slash",
    "has space",
    "a".repeat(65),
  ]) {
    assert.throws(() => validateTicket(invalid), /ticket id/);
  }
  assert.equal(validateTicket("ED-05_portable.v2"), "ED-05_portable.v2");

  const fixture = makeFixture({ ticket: "bad..ref" });
  try {
    fixture.ops.checkRefFormat = () => {
      throw new Error("invalid Git ref");
    };
    assert.equal(fixture.run(["drop", fixture.ticket]), 1);
    assert.match(fixture.errors.join("\n"), /invalid Git ref/);
    assert.deepEqual(fixture.actions, []);
  } finally {
    fixture.dispose();
  }
});

test("strict receipt reader rejects a temporary or oversized receipt", () => {
  const fixture = makeFixture();
  try {
    mkdirSync(fixture.lane.receiptRoot, { recursive: true });
    writeFileSync(fixture.lane.nextReceiptPath, "{}", "utf8");
    assert.equal(
      readPendingReceipt(fixture.lane, fixture.ops).state,
      "invalid",
    );
    unlinkSync(fixture.lane.nextReceiptPath);
    writeFileSync(fixture.lane.receiptPath, "x".repeat(4097), "utf8");
    assert.equal(
      readPendingReceipt(fixture.lane, fixture.ops).state,
      "invalid",
    );
  } finally {
    fixture.dispose();
  }
});

test("worktree porcelain parser consumes NUL records without trusting prose", () => {
  const body = [
    `worktree ${path.resolve("one")}`,
    `HEAD ${commit}`,
    "branch refs/heads/main",
    "",
    `worktree ${path.resolve("two")}`,
    `HEAD ${otherCommit}`,
    "branch refs/heads/lane/TWO",
    "",
    "",
  ].join("\0");
  assert.deepEqual(parseWorktreePorcelain(body), [
    {
      path: path.resolve("one"),
      head: commit,
      branch: "refs/heads/main",
    },
    {
      path: path.resolve("two"),
      head: otherCommit,
      branch: "refs/heads/lane/TWO",
    },
  ]);
});

function git(cwd, args) {
  return execFileSync("git", args, {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

test("real bare remote deletion uses the exact expected-tip lease", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-lane-git-"));
  const remote = path.join(root, "origin.git");
  const repo = path.join(root, "repo");
  try {
    git(root, ["init", "--bare", remote]);
    git(root, ["init", "-b", "main", repo]);
    git(repo, ["config", "user.name", "Lane Test"]);
    git(repo, ["config", "user.email", "lane-test@example.invalid"]);
    writeFileSync(path.join(repo, "file.txt"), "one\n", "utf8");
    git(repo, ["add", "file.txt"]);
    git(repo, ["commit", "-m", "initial"]);
    const first = git(repo, ["rev-parse", "HEAD"]);
    git(repo, ["remote", "add", "origin", remote]);
    git(repo, ["push", "-u", "origin", "main"]);
    git(repo, ["branch", "lane/REMOTE", first]);
    git(repo, ["push", "origin", "lane/REMOTE"]);

    const ops = createSystemOps({ repo, platform: process.platform });
    const ref = "refs/heads/lane/REMOTE";
    assert.equal(ops.remoteTip(ref), first);
    ops.deleteRemoteBranch(ref, first);
    assert.equal(ops.remoteTip(ref), undefined);

    git(repo, ["push", "origin", "lane/REMOTE"]);
    writeFileSync(path.join(repo, "file.txt"), "two\n", "utf8");
    git(repo, ["add", "file.txt"]);
    git(repo, ["commit", "-m", "second"]);
    const second = git(repo, ["rev-parse", "HEAD"]);
    git(repo, ["push", "origin", `HEAD:${ref}`]);
    assert.equal(ops.remoteTip(ref), second);
    assert.throws(() => ops.deleteRemoteBranch(ref, first));
    assert.equal(ops.remoteTip(ref), second);
  } finally {
    removeDirectory(root);
  }
});

test("real Git cleanup resumes after the worktree and local ref are absent", () => {
  const root = realpathSync.native(
    mkdtempSync(path.join(tmpdir(), "sage-lane-resume-")),
  );
  const repo = path.join(root, "repo");
  try {
    git(root, ["init", "-b", "main", repo]);
    git(repo, ["config", "user.name", "Lane Test"]);
    git(repo, ["config", "user.email", "lane-test@example.invalid"]);
    writeFileSync(path.join(repo, ".gitignore"), "work/\n", "utf8");
    git(repo, ["add", ".gitignore"]);
    git(repo, ["commit", "-m", "initial"]);
    const lane = createLaneIdentity({
      repo,
      workRoot: path.join(root, "trees"),
      cargoRoot: path.join(root, "cargo"),
      ticket: "REAL",
      platform: process.platform,
    });
    git(repo, ["worktree", "add", "-b", lane.branch, lane.worktreePath]);
    mkdirSync(lane.cargoTargetPath, { recursive: true });
    const ops = createSystemOps({ repo });
    const tip = ops.revParse("HEAD");
    createReceipt(
      lane,
      { ...receiptDetails(false), integratedCommit: tip, laneCommit: tip },
      ops,
    );
    const remove = ops.removeCargoTarget;
    ops.removeCargoTarget = () => {
      throw new Error("injected cache failure");
    };
    const errors = [];
    const options = {
      repo,
      env: { SAGE_WORK_DIR: lane.workRoot, SAGE_CARGO_DIR: lane.cargoRoot },
      ops,
      output: { log() {}, error: (line) => errors.push(line) },
    };
    assert.equal(runLane(["done", lane.ticket], options), 1);
    assert.match(errors.join("\n"), /failed stage: cargo-cache/);
    assert.equal(ops.refTip(lane.branchRef), undefined);
    assert.equal(existsSync(lane.worktreePath), false);
    ops.removeCargoTarget = remove;
    assert.equal(runLane(["done", lane.ticket], options), 0);
    assert.equal(existsSync(lane.receiptPath), false);
  } finally {
    removeDirectory(root);
  }
});

for (const stage of ["failRemote", "failLocal", "failCache"]) {
  test(`${stage} error after convergence continues to receipt removal`, () => {
    const fixture = makeFixture();
    try {
      installReceipt(fixture);
      fixture.state[stage] = "after";
      assert.equal(fixture.run(["done", fixture.ticket]), 0);
      assert.equal(existsSync(fixture.lane.receiptPath), false);
    } finally {
      fixture.dispose();
    }
  });
}

test("receipt publication inspection failure remains bounded partial success", () => {
  const fixture = makeFixture();
  try {
    const original = fixture.ops.rename;
    fixture.ops.rename = (...args) => {
      original(...args);
      fixture.ops.lstat = () => {
        throw new Error("inspection denied");
      };
      throw new Error("rename reported failure after publication");
    };
    assert.equal(fixture.run(["done", fixture.ticket, "--gate=none"]), 1);
    assert.match(fixture.errors.join("\n"), /LANE PARTIAL SUCCESS/);
    assert.match(fixture.errors.join("\n"), /receipt=unknown/);
    assert.equal(existsSync(fixture.lane.worktreePath), true);
    assert.equal(fixture.actions.includes("worktree-remove"), false);
  } finally {
    fixture.dispose();
  }
});

test("worktree removal leaving an unregistered residual stops later stages", () => {
  const fixture = makeFixture();
  try {
    installReceipt(fixture);
    fixture.ops.removeWorktree = () => {
      fixture.actions.push("worktree-remove");
      fixture.state.worktree.registered = false;
      throw new Error("EPERM after deregistration");
    };
    assert.equal(fixture.run(["done", fixture.ticket]), 1);
    assert.match(fixture.errors.join("\n"), /path=residual/);
    assert.deepEqual(fixture.actions, ["worktree-remove"]);
    assert.equal(existsSync(fixture.lane.receiptPath), true);
  } finally {
    fixture.dispose();
  }
});

test("resume uses recorded no-push policy despite changed CLI flags", () => {
  const fixture = makeFixture();
  try {
    installReceipt(fixture, false);
    fixture.state.remoteError = new Error("remote must not be queried");
    assert.equal(fixture.run(["done", fixture.ticket, "--gate=full"]), 0);
    assert.equal(fixture.state.remoteInspections, 0);
    assert.deepEqual(fixture.actions, [
      "worktree-remove",
      "local-delete",
      "cache-remove",
      "receipt-remove",
    ]);
  } finally {
    fixture.dispose();
  }
});

test("Cargo target symlink authority is rejected without destructive work", () => {
  const fixture = makeFixture();
  try {
    installReceipt(fixture);
    const original = fixture.ops.lstat;
    fixture.ops.lstat = (target) =>
      target === fixture.lane.cargoTargetPath
        ? { isSymbolicLink: () => true, isDirectory: () => true }
        : original(target);
    assert.equal(fixture.run(["done", fixture.ticket]), 1);
    assert.match(fixture.errors.join("\n"), /LANE CLEANUP BLOCKED/);
    assert.deepEqual(fixture.actions, []);
  } finally {
    fixture.dispose();
  }
});

test("fast-forward error with converged main proceeds to publication", () => {
  const fixture = makeFixture();
  try {
    fixture.ops.mergeBranch = () => {
      fixture.actions.push("merge");
      throw new Error("post-merge error");
    };
    assert.equal(fixture.run(["done", fixture.ticket, "--gate=fast"]), 0);
    assert.equal(fixture.state.lastReceipt.integratedCommit, commit);
  } finally {
    fixture.dispose();
  }
});

test("importing the lane module has no CLI side effects", async () => {
  const before = process.exitCode;
  await import(`./lane.mjs?guard=${Date.now()}`);
  assert.equal(process.exitCode, before);
});
