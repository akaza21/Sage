import path from "node:path";

export const RECEIPT_VERSION = 1;
export const MAX_TICKET_LENGTH = 64;
export const MAX_RECEIPT_BYTES = 4096;
const commitPattern = /^[0-9a-f]{40}$/;
const ticketPattern = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;
const devicePattern = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i;

export class ReceiptError extends Error {
  constructor(message) {
    super(message);
    this.name = "ReceiptError";
  }
}

export function validateTicket(ticket) {
  if (
    typeof ticket !== "string" ||
    ticket.length < 1 ||
    ticket.length > MAX_TICKET_LENGTH ||
    !ticketPattern.test(ticket) ||
    ticket.endsWith(".") ||
    devicePattern.test(ticket)
  ) {
    throw new Error(
      "ticket id must be 1-64 portable characters, start alphanumeric, and not name a Windows device",
    );
  }
  return ticket;
}

function comparable(value, platform) {
  const normalized = path.resolve(value);
  return platform === "win32" ? normalized.toLowerCase() : normalized;
}

export function assertContained(root, candidate, label, platform) {
  const rootPath = path.resolve(root);
  const candidatePath = path.resolve(candidate);
  const relative = path.relative(rootPath, candidatePath);
  const escaped =
    relative === "" ||
    relative === ".." ||
    relative.startsWith(`..${path.sep}`) ||
    path.isAbsolute(relative);
  if (escaped) {
    throw new Error(`${label} is not contained beneath its configured root`);
  }
  if (
    comparable(path.join(rootPath, relative), platform) !==
    comparable(candidatePath, platform)
  ) {
    throw new Error(`${label} does not resolve to its configured path`);
  }
  return candidatePath;
}

export function createLaneIdentity({
  repo,
  workRoot,
  cargoRoot,
  ticket,
  platform,
}) {
  validateTicket(ticket);
  const canonicalRepo = path.resolve(repo);
  const configuredWorkRoot = path.resolve(workRoot);
  const configuredCargoRoot = path.resolve(cargoRoot);
  const receiptRoot = path.join(canonicalRepo, "work", "lane-receipts");
  const branch = `lane/${ticket}`;
  const worktreePath = assertContained(
    configuredWorkRoot,
    path.join(configuredWorkRoot, ticket),
    "worktree path",
    platform,
  );
  const cargoTargetPath = assertContained(
    configuredCargoRoot,
    path.join(configuredCargoRoot, ticket),
    "Cargo target path",
    platform,
  );
  const receiptPath = assertContained(
    receiptRoot,
    path.join(receiptRoot, `${ticket}.json`),
    "receipt path",
    platform,
  );
  const nextReceiptPath = assertContained(
    receiptRoot,
    path.join(receiptRoot, `${ticket}.next.json`),
    "temporary receipt path",
    platform,
  );
  return {
    repo: canonicalRepo,
    workRoot: configuredWorkRoot,
    cargoRoot: configuredCargoRoot,
    receiptRoot,
    ticket,
    branch,
    branchRef: `refs/heads/${branch}`,
    remoteBranchRef: `refs/heads/${branch}`,
    worktreePath,
    cargoTargetPath,
    receiptPath,
    nextReceiptPath,
    platform,
  };
}

function exactKeys(value, keys, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new ReceiptError(`${label} must be an object`);
  }
  const actual = Object.keys(value).sort();
  const expected = [...keys].sort();
  if (
    actual.length !== expected.length ||
    actual.some((key, index) => key !== expected[index])
  ) {
    throw new ReceiptError(`${label} has an unsupported shape`);
  }
}

export function validateReceipt(value, lane) {
  exactKeys(
    value,
    [
      "version",
      "kind",
      "ticket",
      "branch",
      "integratedCommit",
      "laneCommit",
      "worktreePath",
      "cargoTargetPath",
      "gate",
      "push",
    ],
    "receipt",
  );
  exactKeys(value.gate, ["mode", "status"], "receipt gate");
  exactKeys(value.push, ["requested", "succeeded"], "receipt push");
  if (value.version !== RECEIPT_VERSION || value.kind !== "lane-cleanup") {
    throw new ReceiptError("receipt version is unsupported");
  }
  if (value.ticket !== lane.ticket || value.branch !== lane.branch) {
    throw new ReceiptError(
      "receipt ticket or branch does not match the requested lane",
    );
  }
  if (
    value.worktreePath !== lane.worktreePath ||
    value.cargoTargetPath !== lane.cargoTargetPath
  ) {
    throw new ReceiptError(
      "receipt paths do not match the current configured roots",
    );
  }
  assertContained(
    lane.workRoot,
    value.worktreePath,
    "receipt worktree path",
    lane.platform,
  );
  assertContained(
    lane.cargoRoot,
    value.cargoTargetPath,
    "receipt Cargo target path",
    lane.platform,
  );
  if (
    !commitPattern.test(value.integratedCommit) ||
    !commitPattern.test(value.laneCommit)
  ) {
    throw new ReceiptError(
      "receipt commits must be full lowercase 40-hex object ids",
    );
  }
  if (value.integratedCommit !== value.laneCommit) {
    throw new ReceiptError(
      "receipt commits do not describe a fast-forward integration",
    );
  }
  if (!new Set(["fast", "full", "none"]).has(value.gate.mode)) {
    throw new ReceiptError("receipt gate mode is invalid");
  }
  const expectedGateStatus = value.gate.mode === "none" ? "not-run" : "passed";
  if (value.gate.status !== expectedGateStatus) {
    throw new ReceiptError("receipt gate status is invalid");
  }
  if (
    typeof value.push.requested !== "boolean" ||
    typeof value.push.succeeded !== "boolean"
  ) {
    throw new ReceiptError("receipt push truth must be boolean");
  }
  if (value.push.succeeded !== value.push.requested) {
    throw new ReceiptError("receipt push truth is inconsistent");
  }
  return Object.freeze(value);
}

function lstatMaybe(ops, target) {
  try {
    return ops.lstat(target);
  } catch (error) {
    if (error?.code === "ENOENT") return undefined;
    throw error;
  }
}

export function receiptPresence(lane, ops) {
  return lstatMaybe(ops, lane.receiptPath) ||
    lstatMaybe(ops, lane.nextReceiptPath)
    ? "present"
    : "absent";
}

export function readPendingReceipt(lane, ops) {
  const stat = lstatMaybe(ops, lane.receiptPath);
  const nextStat = lstatMaybe(ops, lane.nextReceiptPath);
  if (!stat && !nextStat) return { state: "absent" };
  try {
    assertExistingReceiptDirectory(ops, lane);
  } catch (error) {
    return { state: "invalid", error };
  }
  if (nextStat) {
    return {
      state: "invalid",
      error: new ReceiptError(
        `stale temporary receipt: ${lane.nextReceiptPath}; see docs/development/WORKFLOW.md#lane-cleanup-recovery`,
      ),
    };
  }
  if (!stat.isFile() || stat.isSymbolicLink()) {
    return {
      state: "invalid",
      error: new ReceiptError("receipt must be a regular non-link file"),
    };
  }
  if (stat.size < 2 || stat.size > MAX_RECEIPT_BYTES) {
    return {
      state: "invalid",
      error: new ReceiptError("receipt size is outside the bounded limit"),
    };
  }
  try {
    const body = ops.readBounded(lane.receiptPath, MAX_RECEIPT_BYTES + 1);
    if (
      Buffer.byteLength(body, "utf8") !== stat.size ||
      Buffer.byteLength(body, "utf8") > MAX_RECEIPT_BYTES
    ) {
      throw new ReceiptError("receipt changed while it was read");
    }
    const receipt = validateReceipt(JSON.parse(body), lane);
    if (body !== `${JSON.stringify(receipt, null, 2)}\n`) {
      throw new ReceiptError("receipt JSON formatting is not supported");
    }
    return { state: "valid", receipt };
  } catch (error) {
    return {
      state: "invalid",
      error:
        error instanceof ReceiptError
          ? error
          : new ReceiptError("receipt JSON is malformed"),
    };
  }
}

function assertExistingReceiptDirectory(ops, lane) {
  const parent = ops.lstat(path.dirname(lane.receiptRoot));
  if (!parent.isDirectory() || parent.isSymbolicLink()) {
    throw new ReceiptError(
      "receipt parent must be a regular non-link directory",
    );
  }
  const stat = ops.lstat(lane.receiptRoot);
  if (!stat.isDirectory() || stat.isSymbolicLink()) {
    throw new ReceiptError(
      "receipt directory must be a regular non-link directory",
    );
  }
  const physicalRepo = ops.realpath(lane.repo);
  const physicalReceipts = ops.realpath(lane.receiptRoot);
  try {
    assertContained(
      physicalRepo,
      physicalReceipts,
      "physical receipt directory",
      lane.platform,
    );
  } catch {
    throw new ReceiptError(
      "receipt directory is not physically beneath the canonical checkout",
    );
  }
}

function assertReceiptDirectory(ops, lane) {
  const workPath = path.dirname(lane.receiptRoot);
  const workState = lstatMaybe(ops, workPath);
  if (workState && (!workState.isDirectory() || workState.isSymbolicLink())) {
    throw new ReceiptError(
      "receipt parent must be a regular non-link directory",
    );
  }
  ops.mkdir(lane.receiptRoot);
  assertExistingReceiptDirectory(ops, lane);
}

export function createReceipt(lane, details, ops) {
  const receipt = validateReceipt(
    {
      version: RECEIPT_VERSION,
      kind: "lane-cleanup",
      ticket: lane.ticket,
      branch: lane.branch,
      integratedCommit: details.integratedCommit,
      laneCommit: details.laneCommit,
      worktreePath: lane.worktreePath,
      cargoTargetPath: lane.cargoTargetPath,
      gate: { mode: details.gateMode, status: details.gateStatus },
      push: {
        requested: details.pushRequested,
        succeeded: details.pushSucceeded,
      },
    },
    lane,
  );
  const body = `${JSON.stringify(receipt, null, 2)}\n`;
  if (Buffer.byteLength(body, "utf8") > MAX_RECEIPT_BYTES) {
    throw new ReceiptError("receipt exceeds the bounded limit");
  }
  assertReceiptDirectory(ops, lane);
  if (receiptPresence(lane, ops) !== "absent") {
    throw new ReceiptError("a lane receipt already exists");
  }
  ops.writeExclusive(lane.nextReceiptPath, body);
  try {
    ops.rename(lane.nextReceiptPath, lane.receiptPath);
  } catch (error) {
    try {
      ops.unlink(lane.nextReceiptPath);
    } catch {
      // The exclusive temporary file remains a fail-closed retry barrier.
    }
    throw error;
  }
  const pending = readPendingReceipt(lane, ops);
  if (
    pending.state !== "valid" ||
    JSON.stringify(pending.receipt) !== JSON.stringify(receipt)
  ) {
    throw new ReceiptError("atomic receipt verification failed");
  }
  return receipt;
}

export function parseWorktreePorcelain(body) {
  if (
    typeof body !== "string" ||
    Buffer.byteLength(body, "utf8") > 1024 * 1024
  ) {
    throw new Error("worktree inventory is not bounded text");
  }
  const records = [];
  let current;
  for (const field of body.split("\0")) {
    if (field === "") {
      if (current) records.push(current);
      current = undefined;
      continue;
    }
    const separator = field.indexOf(" ");
    const key = separator === -1 ? field : field.slice(0, separator);
    const value = separator === -1 ? true : field.slice(separator + 1);
    if (key === "worktree") {
      if (current) throw new Error("worktree inventory record is malformed");
      current = { path: value };
    } else {
      if (!current || Object.hasOwn(current, key)) {
        throw new Error("worktree inventory record is malformed");
      }
      if (key === "HEAD") current.head = value;
      else if (key === "branch") current.branch = value;
      else if (
        key === "detached" ||
        key === "bare" ||
        key === "prunable" ||
        key === "locked"
      ) {
        current[key] = value;
      }
    }
  }
  if (current) records.push(current);
  for (const record of records) {
    if (
      typeof record.path !== "string" ||
      (record.head && !commitPattern.test(record.head))
    ) {
      throw new Error("worktree inventory contains an invalid record");
    }
  }
  return records;
}

export function samePath(left, right, platform) {
  return comparable(left, platform) === comparable(right, platform);
}
