import { createHash, randomUUID } from "node:crypto";
import { mkdir, open, readFile, realpath, unlink } from "node:fs/promises";
import path from "node:path";

export class CargoLeaseError extends Error {
  constructor(message, { code, cause } = {}) {
    super(message, { cause });
    this.name = "CargoLeaseError";
    this.code = code;
  }
}

const DEFAULT_OPERATIONS = Object.freeze({
  mkdir,
  open,
  readFile,
  realpath,
  unlink,
});

function abortedError(signal) {
  return new CargoLeaseError(
    "Cargo target lease acquisition exceeded its lane deadline",
    {
      code: "cargo-target-lease-timeout",
      cause: signal.reason,
    },
  );
}

function withSignal(promise, signal, onLateResolution) {
  if (!signal) return promise;
  if (signal.aborted) return Promise.reject(abortedError(signal));
  return new Promise((resolve, reject) => {
    let finished = false;
    const abort = () => {
      if (finished) return;
      finished = true;
      reject(abortedError(signal));
    };
    signal.addEventListener("abort", abort, { once: true });
    promise.then(
      (value) => {
        if (finished) {
          void onLateResolution?.(value);
          return;
        }
        finished = true;
        signal.removeEventListener("abort", abort);
        resolve(value);
      },
      (error) => {
        if (finished) return;
        finished = true;
        signal.removeEventListener("abort", abort);
        reject(error);
      },
    );
  });
}

function resolvedTarget(targetDirectory, repositoryRoot) {
  if (typeof targetDirectory !== "string" || targetDirectory.length === 0) {
    throw new CargoLeaseError("Cargo target directory is required", {
      code: "cargo-target-invalid",
    });
  }
  if (typeof repositoryRoot !== "string" || repositoryRoot.length === 0) {
    throw new CargoLeaseError("Cargo target repository root is required", {
      code: "cargo-target-invalid",
    });
  }
  return path.resolve(repositoryRoot, targetDirectory);
}

async function canonicalCargoTargetDetails(
  targetDirectory,
  {
    repositoryRoot = process.cwd(),
    signal,
    operations: providedOperations = {},
  } = {},
) {
  const operations = { ...DEFAULT_OPERATIONS, ...providedOperations };
  let cursor = resolvedTarget(targetDirectory, repositoryRoot);
  const remainder = [];
  while (true) {
    try {
      const physical = await withSignal(operations.realpath(cursor), signal);
      return {
        physicalTarget: path.normalize(path.join(physical, ...remainder)),
        leaseParent: remainder.length === 0 ? path.dirname(physical) : physical,
      };
    } catch (error) {
      if (error.code === "cargo-target-lease-timeout") throw error;
      if (error.code !== "ENOENT") {
        throw new CargoLeaseError("Cargo target canonicalization failed", {
          code: "cargo-target-canonicalization-failed",
          cause: error,
        });
      }
      const parent = path.dirname(cursor);
      if (parent === cursor) {
        throw new CargoLeaseError("Cargo target has no canonical ancestor", {
          code: "cargo-target-canonicalization-failed",
          cause: error,
        });
      }
      remainder.unshift(path.basename(cursor));
      cursor = parent;
    }
  }
}

export async function canonicalCargoTarget(targetDirectory, options = {}) {
  return (await canonicalCargoTargetDetails(targetDirectory, options))
    .physicalTarget;
}

function identityForPhysicalTarget(
  physicalTarget,
  platform = process.platform,
) {
  const canonical =
    platform === "win32" || platform === "darwin"
      ? physicalTarget.toLowerCase()
      : physicalTarget;
  return createHash("sha256").update(canonical).digest("hex");
}

export async function cargoLeasePath(targetDirectory, options = {}) {
  const { physicalTarget, leaseParent } = await canonicalCargoTargetDetails(
    targetDirectory,
    options,
  );
  return path.join(
    leaseParent,
    ".sage-cargo-leases",
    `${identityForPhysicalTarget(physicalTarget, options.platform)}.lock`,
  );
}

export async function acquireCargoTargetLease(
  targetDirectory,
  {
    repositoryRoot = process.cwd(),
    signal,
    operations: providedOperations = {},
    platform = process.platform,
  } = {},
) {
  const operations = { ...DEFAULT_OPERATIONS, ...providedOperations };
  const requestedTarget = resolvedTarget(targetDirectory, repositoryRoot);
  try {
    await withSignal(
      operations.mkdir(requestedTarget, { recursive: true }),
      signal,
    );
  } catch (error) {
    if (error.code === "cargo-target-lease-timeout") throw error;
    throw new CargoLeaseError("Cargo target materialization failed", {
      code: "cargo-target-materialization-failed",
      cause: error,
    });
  }
  const { physicalTarget, leaseParent } = await canonicalCargoTargetDetails(
    requestedTarget,
    {
      repositoryRoot,
      signal,
      operations,
    },
  );
  const lockPath = path.join(
    leaseParent,
    ".sage-cargo-leases",
    `${identityForPhysicalTarget(physicalTarget, platform)}.lock`,
  );
  await withSignal(
    operations.mkdir(path.dirname(lockPath), { recursive: true }),
    signal,
  );
  let handle;
  try {
    const opening = operations.open(lockPath, "wx", 0o600);
    handle = await withSignal(opening, signal, async (lateHandle) => {
      await lateHandle.close().catch(() => {});
      await operations.unlink(lockPath).catch(() => {});
    });
  } catch (error) {
    if (error.code === "cargo-target-lease-timeout") throw error;
    if (error.code !== "EEXIST") throw error;
    let owner = "unreadable owner";
    try {
      owner = await withSignal(operations.readFile(lockPath, "utf8"), signal);
    } catch (readError) {
      if (readError.code === "cargo-target-lease-timeout") throw readError;
    }
    throw new CargoLeaseError(
      `Cargo target is leased or quarantined (${lockPath}). Do not remove it until the recorded owner is authenticated dead and every descendant using the target is independently proven absent. Owner: ${owner}`,
      { code: "cargo-target-locked", cause: error },
    );
  }
  const acquiredOwner = {
    version: 1,
    pid: process.pid,
    nonce: randomUUID(),
    processStartedAt: new Date(
      Date.now() - Math.round(process.uptime() * 1_000),
    ).toISOString(),
    target: physicalTarget,
    acquiredAt: new Date().toISOString(),
    quarantined: false,
  };
  const acquiredBytes = JSON.stringify(acquiredOwner);
  try {
    await withSignal(handle.writeFile(acquiredBytes, "utf8"), signal);
    await withSignal(handle.sync(), signal);
  } catch (error) {
    await handle.close().catch(() => {});
    await operations.unlink(lockPath).catch(() => {});
    throw error;
  }
  let closed = false;
  async function closeHandle() {
    if (closed) return;
    closed = true;
    await handle.close();
  }
  return {
    path: lockPath,
    async quarantine(failure) {
      if (closed) return;
      const quarantinedOwner = JSON.stringify({
        ...acquiredOwner,
        quarantined: true,
        quarantinedAt: new Date().toISOString(),
        failureCode: failure?.code ?? "settlement-unconfirmed",
        recovery:
          "Manual recovery only after the recorded owner is authenticated dead and every descendant using this target is independently proven absent.",
      });
      try {
        await handle.truncate(0);
        await handle.write(quarantinedOwner, 0, "utf8");
        await handle.sync();
      } finally {
        await closeHandle();
      }
    },
    async release() {
      if (closed) return;
      try {
        const current = await operations.readFile(lockPath, "utf8");
        if (current !== acquiredBytes) {
          throw new CargoLeaseError("Cargo target lease owner changed", {
            code: "cargo-target-lease-owner-changed",
          });
        }
        await closeHandle();
        await operations.unlink(lockPath);
      } catch (error) {
        await closeHandle().catch(() => {});
        throw error;
      }
    },
  };
}
