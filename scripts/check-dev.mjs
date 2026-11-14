import { testFeatures } from "./cargo-features.mjs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  buildRustEnvironment,
  executableName,
  runCliPlan,
} from "./lib/check-process.mjs";
import {
  boundedCommand,
  COMMAND_DEADLINES_MS,
  npmCommand,
  resolveNpmInvocation,
  VALIDATION_DEADLINES_MS,
} from "./lib/check-plans.mjs";

const scriptPath = fileURLToPath(import.meta.url);
const repositoryRoot = path.resolve(path.dirname(scriptPath), "..");
const MAX_TARGETS = 16;

function boundedTarget(value, option) {
  const normalized = value?.replaceAll("\\", "/").replace(/\/+$/u, "");
  if (
    typeof value !== "string" ||
    value.trim() !== value ||
    value.length < 1 ||
    value.length > 512 ||
    value.startsWith("-") ||
    value === "." ||
    value === "*" ||
    value === "**" ||
    (option === "--vitest" &&
      (value.includes("..") ||
        value.includes("**") ||
        ["src", "scripts", "tests"].includes(normalized)))
  ) {
    throw new Error(`${option} requires a bounded value`);
  }
  if (
    option === "--rust-filter" &&
    !/^[a-zA-Z0-9_]+(?:::[a-zA-Z0-9_]+)+$/u.test(value)
  ) {
    throw new Error("--rust-filter requires a concrete module::test path");
  }
  if (
    option === "--vitest" &&
    !/(?:^|\/)[^/]+\.(?:test|spec)\.[cm]?[jt]sx?$/u.test(normalized)
  ) {
    throw new Error("--vitest requires a concrete test/spec file path");
  }
  return value;
}

export function parseDevArguments(arguments_) {
  const options = { dryRun: false, rustFilters: [], vitestTargets: [] };
  for (let index = 0; index < arguments_.length; index += 1) {
    const argument = arguments_[index];
    if (argument === "--dry-run") options.dryRun = true;
    else if (argument === "--rust-filter") {
      options.rustFilters.push(
        boundedTarget(arguments_[index + 1], "--rust-filter"),
      );
      index += 1;
    } else if (argument === "--vitest") {
      options.vitestTargets.push(
        boundedTarget(arguments_[index + 1], "--vitest"),
      );
      index += 1;
    } else throw new Error(`unknown developer-check argument: ${argument}`);
  }
  const count = options.rustFilters.length + options.vitestTargets.length;
  if (count === 0)
    throw new Error("developer check requires at least one explicit target");
  if (count > MAX_TARGETS)
    throw new Error(`developer check accepts at most ${MAX_TARGETS} targets`);
  return options;
}

export function createDevPlan(
  options,
  {
    platform = process.platform,
    repositoryRoot: root = repositoryRoot,
    rustEnvironment = buildRustEnvironment({ repositoryRoot: root, platform }),
    npmInvocation = resolveNpmInvocation({ platform }),
  } = {},
) {
  const groups = [];
  const prerequisiteGroups = [];
  if (options.rustFilters.length > 0) {
    prerequisiteGroups.push({
      label: "frontend-bundle-prerequisite",
      concurrency: 1,
      timeoutMs: VALIDATION_DEADLINES_MS.devLane,
      commands: [
        npmCommand({
          label: "developer:frontend-build",
          npmArguments: ["run", "build"],
          repositoryRoot: root,
          timeoutMs: COMMAND_DEADLINES_MS.build,
          npmInvocation,
        }),
      ],
    });
    groups.push({
      label: "rust-focused",
      concurrency: 1,
      timeoutMs: VALIDATION_DEADLINES_MS.devLane,
      cargoTargetDir: rustEnvironment.CARGO_TARGET_DIR,
      repositoryRoot: root,
      commands: options.rustFilters.map((filter) =>
        boundedCommand(
          {
            label: `rust:${filter}`,
            command: executableName("cargo", platform),
            args: [
              "test",
              "--workspace",
              "--features",
              testFeatures(platform).join(","),
              filter,
            ],
            cwd: root,
            env: rustEnvironment,
          },
          COMMAND_DEADLINES_MS.focusedTest,
        ),
      ),
    });
  }
  if (options.vitestTargets.length > 0) {
    groups.push({
      label: "frontend-focused",
      concurrency: 1,
      timeoutMs: VALIDATION_DEADLINES_MS.devLane,
      commands: options.vitestTargets.map((target) =>
        npmCommand({
          label: `vitest:${target}`,
          npmArguments: ["run", "test", "--", target],
          repositoryRoot: root,
          timeoutMs: COMMAND_DEADLINES_MS.focusedTest,
          npmInvocation,
        }),
      ),
    });
  }
  return { mode: "developer", prerequisiteGroups, groups };
}

async function main() {
  const options = parseDevArguments(process.argv.slice(2));
  await runCliPlan(createDevPlan(options), options);
}

if (process.argv[1] && path.resolve(process.argv[1]) === scriptPath) {
  main().catch((error) => {
    process.stderr.write(`check-dev: ${error.message}\n`);
    process.exitCode = 1;
  });
}
