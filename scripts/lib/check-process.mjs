import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import path from "node:path";

import { acquireCargoTargetLease } from "./check-cargo-lease.mjs";
import { CheckProcessError, executeCommand } from "./check-child-process.mjs";
import { executeWindowsCargoJobCommand } from "./check-windows-cargo-job.mjs";

export { CheckProcessError, executeCommand };

const MAX_COMMANDS = 32;

export function executableName(name, platform = process.platform) {
  if (platform === "win32" && name === "cargo") return "cargo.exe";
  return name;
}

export function findExecutable(
  name,
  { environment = process.env, platform = process.platform } = {},
) {
  const pathEntries = (environment.PATH ?? "")
    .split(path.delimiter)
    .filter(Boolean);
  const extensions =
    platform === "win32"
      ? (environment.PATHEXT ?? ".EXE;.CMD;.BAT;.COM").split(";")
      : [""];
  for (const directory of pathEntries) {
    for (const extension of extensions) {
      for (const suffix of new Set([extension, extension.toLowerCase()])) {
        const candidate = path.join(directory, `${name}${suffix}`);
        if (existsSync(candidate)) return candidate;
      }
    }
  }
  return undefined;
}

function repositoryIdentity(repositoryRoot) {
  return createHash("sha256")
    .update(path.resolve(repositoryRoot).toLowerCase())
    .digest("hex")
    .slice(0, 12);
}

export function buildRustEnvironment({
  platform = process.platform,
  environment = process.env,
  repositoryRoot,
  sccachePath,
  windowsDDriveAvailable = platform === "win32" && existsSync("D:\\"),
}) {
  const selectedTarget =
    environment.SAGE_CARGO_TARGET_DIR ??
    (windowsDDriveAvailable
      ? path.join(
          "C:\\SageCaches\\cargo-target",
          `${path.basename(repositoryRoot)}-${repositoryIdentity(repositoryRoot)}`,
        )
      : path.join(repositoryRoot, "src-tauri", "target"));
  const selectedSccache =
    sccachePath ?? findExecutable("sccache", { environment, platform });
  const result = { CARGO_TARGET_DIR: selectedTarget };
  if (!selectedSccache) return result;

  result.RUSTC_WRAPPER = selectedSccache;
  result.SCCACHE_CACHE_SIZE = "20G";
  const selectedCache =
    environment.SAGE_SCCACHE_DIR ??
    (windowsDDriveAvailable ? "C:\\SageCaches\\sccache" : undefined);
  if (selectedCache) result.SCCACHE_DIR = selectedCache;
  return result;
}

function laneSettlementCap(group) {
  return Math.max(
    1,
    ...group.commands.map(
      ({ terminationGraceMs = 0, settlementTimeoutMs = 0 }) =>
        terminationGraceMs + settlementTimeoutMs,
    ),
  );
}

async function runGroup(group, parentSignal, execute, executeWindowsCargo) {
  if (!Number.isInteger(group.concurrency) || group.concurrency < 1) {
    throw new Error(`${group.label} has an invalid concurrency cap`);
  }
  if (!Array.isArray(group.commands) || group.commands.length > MAX_COMMANDS) {
    throw new Error(`${group.label} exceeds the bounded command count`);
  }
  if (!Number.isSafeInteger(group.timeoutMs) || group.timeoutMs < 1) {
    throw new Error(`${group.label} has an invalid lane deadline`);
  }
  const laneController = new AbortController();
  const parentAbort = () => laneController.abort(parentSignal.reason);
  parentSignal.addEventListener("abort", parentAbort, { once: true });
  const laneError = new CheckProcessError(
    `${group.label} lane exceeded its deadline`,
    { code: "lane-timeout" },
  );
  const laneTimer = setTimeout(
    () => laneController.abort(laneError),
    group.timeoutMs,
  );
  let nextIndex = 0;
  let lease;
  let retainLease = false;
  const worker = async () => {
    while (!laneController.signal.aborted) {
      const index = nextIndex;
      nextIndex += 1;
      if (index >= group.commands.length) return;
      const command = group.commands[index];
      const runner =
        process.platform === "win32" && group.cargoTargetDir
          ? executeWindowsCargo
          : execute;
      await runner(command, laneController.signal, {
        repositoryRoot: group.repositoryRoot,
      });
    }
  };
  const workerCount = Math.min(group.concurrency, group.commands.length);
  try {
    if (group.cargoTargetDir) {
      lease = await acquireCargoTargetLease(group.cargoTargetDir, {
        repositoryRoot: group.repositoryRoot,
        signal: laneController.signal,
      });
    }
    const workers = Array.from({ length: workerCount }, () =>
      worker().catch((error) => {
        laneController.abort(error);
        throw error;
      }),
    );
    const settledWorkers = Promise.allSettled(workers).then((results) => {
      const failures = results.filter(({ status }) => status === "rejected");
      const failure =
        failures.find(({ reason }) => reason?.settled === false) ?? failures[0];
      if (failure) throw failure.reason;
    });
    const hardStop = new Promise((_, reject) => {
      laneController.signal.addEventListener(
        "abort",
        () => {
          const timer = setTimeout(
            () =>
              reject(
                new CheckProcessError(
                  `${group.label} command settlement remained unconfirmed`,
                  {
                    code: "lane-settlement-unconfirmed",
                    cause: laneController.signal.reason ?? laneError,
                    settled: false,
                  },
                ),
              ),
            laneSettlementCap(group),
          );
          timer.unref();
        },
        { once: true },
      );
    });
    await Promise.race([settledWorkers, hardStop]);
    if (laneController.signal.reason === laneError) throw laneError;
  } catch (error) {
    if (lease && error?.settled === false) {
      retainLease = true;
      let cause = error;
      try {
        await lease.quarantine(error);
      } catch (quarantineError) {
        cause = new AggregateError(
          [error, quarantineError],
          "process settlement and Cargo quarantine recording both failed",
        );
      }
      throw new CheckProcessError(
        `${group.label} could not prove process-tree settlement; its Cargo target remains quarantined at ${lease.path}`,
        {
          code: "cargo-target-settlement-unconfirmed",
          cause,
          settled: false,
        },
      );
    }
    throw error;
  } finally {
    clearTimeout(laneTimer);
    parentSignal.removeEventListener("abort", parentAbort);
    if (!retainLease) await lease?.release();
  }
}

export async function runCommandGroups(
  groups,
  {
    execute = executeCommand,
    executeWindowsCargo = executeWindowsCargoJobCommand,
  } = {},
) {
  const controller = new AbortController();
  let primaryError;
  const interrupt = () => {
    primaryError ??= new CheckProcessError("validation interrupted", {
      code: "validation-aborted",
    });
    controller.abort(primaryError);
  };
  process.once("SIGINT", interrupt);
  process.once("SIGTERM", interrupt);
  try {
    await Promise.allSettled(
      groups.map((group) =>
        runGroup(group, controller.signal, execute, executeWindowsCargo).catch(
          (error) => {
            primaryError ??= error;
            controller.abort(primaryError);
            throw error;
          },
        ),
      ),
    );
    if (primaryError) throw primaryError;
  } finally {
    process.removeListener("SIGINT", interrupt);
    process.removeListener("SIGTERM", interrupt);
  }
}

function printableGroups(groups) {
  return groups.map((group) => ({
    label: group.label,
    concurrency: group.concurrency,
    timeoutMs: group.timeoutMs,
    cargoTargetDir: group.cargoTargetDir,
    repositoryRoot: group.repositoryRoot,
    commands: group.commands.map(
      ({
        label,
        command,
        args,
        cwd,
        env = {},
        timeoutMs,
        terminationGraceMs,
        settlementTimeoutMs,
        maximumLineBytes,
        maximumOutputBytes,
      }) => ({
        label,
        command,
        args,
        cwd,
        env,
        timeoutMs,
        terminationGraceMs,
        settlementTimeoutMs,
        maximumLineBytes,
        maximumOutputBytes,
      }),
    ),
  }));
}

export function printablePlan(plan) {
  return {
    mode: plan.mode,
    prerequisiteGroups: printableGroups(plan.prerequisiteGroups ?? []),
    groups: printableGroups(plan.groups),
  };
}

export async function runCliPlan(plan, { dryRun }) {
  if (dryRun) {
    process.stdout.write(`${JSON.stringify(printablePlan(plan), null, 2)}\n`);
    return;
  }
  if (plan.prerequisiteGroups?.length > 0) {
    await runCommandGroups(plan.prerequisiteGroups);
  }
  await runCommandGroups(plan.groups);
}
