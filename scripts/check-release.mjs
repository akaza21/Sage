import path from "node:path";
import { fileURLToPath } from "node:url";

import { buildRustEnvironment, runCliPlan } from "./lib/check-process.mjs";
import {
  COMMAND_DEADLINES_MS,
  createRustValidationCommands,
  npmCommand,
  resolveNpmInvocation,
  VALIDATION_DEADLINES_MS,
} from "./lib/check-plans.mjs";

const scriptPath = fileURLToPath(import.meta.url);
const repositoryRoot = path.resolve(path.dirname(scriptPath), "..");

export function parseReleaseArguments(arguments_) {
  let mode;
  let dryRun = false;
  for (const argument of arguments_) {
    if (argument === "--dry-run") dryRun = true;
    else if (argument === "--validation-only") {
      if (mode)
        throw new Error("release check accepts exactly one explicit mode");
      mode = argument.slice(2);
    } else throw new Error(`unknown release-check argument: ${argument}`);
  }
  if (!mode) throw new Error("release check requires an explicit mode");
  return { dryRun, mode };
}

export function createReleasePlan(
  options,
  {
    platform = process.platform,
    repositoryRoot: root = repositoryRoot,
    rustEnvironment = buildRustEnvironment({ repositoryRoot: root, platform }),
    npmInvocation = resolveNpmInvocation({ platform }),
  } = {},
) {
  const commands = [
    npmCommand({
      label: "release:frontend-check",
      npmArguments: ["run", "check"],
      repositoryRoot: root,
      timeoutMs: COMMAND_DEADLINES_MS.frontendCheck,
      npmInvocation,
    }),
    ...createRustValidationCommands({
      labelPrefix: "release",
      platform,
      repositoryRoot: root,
      rustEnvironment,
    }),
  ];
  return {
    mode: `release:${options.mode}`,
    groups: [
      {
        label: "release",
        concurrency: 1,
        timeoutMs: VALIDATION_DEADLINES_MS.releaseLane,
        cargoTargetDir: rustEnvironment.CARGO_TARGET_DIR,
        repositoryRoot: root,
        commands,
      },
    ],
  };
}

async function main() {
  const options = parseReleaseArguments(process.argv.slice(2));
  await runCliPlan(createReleasePlan(options), options);
}

if (process.argv[1] && path.resolve(process.argv[1]) === scriptPath) {
  main().catch((error) => {
    process.stderr.write(`check-release: ${error.message}\n`);
    process.exitCode = 1;
  });
}
