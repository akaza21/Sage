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

export function parseIntegrationArguments(arguments_) {
  if (arguments_.length === 0) return { dryRun: false };
  if (arguments_.length === 1 && arguments_[0] === "--dry-run") {
    return { dryRun: true };
  }
  throw new Error("integration check accepts only --dry-run");
}

export function createIntegrationPlan({
  platform = process.platform,
  repositoryRoot: root = repositoryRoot,
  rustEnvironment = buildRustEnvironment({ repositoryRoot: root, platform }),
  npmInvocation = resolveNpmInvocation({ platform }),
} = {}) {
  const frontend = [
    ["frontend:format", ["run", "format:check"], COMMAND_DEADLINES_MS.format],
    ["frontend:lint", ["run", "lint"], COMMAND_DEADLINES_MS.lint],
    ["frontend:vitest", ["run", "test"], COMMAND_DEADLINES_MS.frontendTests],
    ["frontend:budgets", ["run", "test:budgets"], COMMAND_DEADLINES_MS.budgets],
    ["frontend:policy", ["run", "check:policy"], COMMAND_DEADLINES_MS.policy],
  ].map(([label, npmArguments, timeoutMs]) =>
    npmCommand({
      label,
      npmArguments,
      repositoryRoot: root,
      timeoutMs,
      npmInvocation,
    }),
  );
  const rust = createRustValidationCommands({
    labelPrefix: "integration",
    platform,
    repositoryRoot: root,
    rustEnvironment,
  });
  return {
    mode: "integration",
    prerequisiteGroups: [
      {
        label: "frontend-bundle-prerequisite",
        concurrency: 1,
        timeoutMs: VALIDATION_DEADLINES_MS.integrationFrontendLane,
        commands: [
          npmCommand({
            label: "frontend:build",
            npmArguments: ["run", "build"],
            repositoryRoot: root,
            timeoutMs: COMMAND_DEADLINES_MS.build,
            npmInvocation,
          }),
          npmCommand({
            label: "frontend:web-bundle",
            npmArguments: ["run", "test:web-bundle"],
            repositoryRoot: root,
            timeoutMs: COMMAND_DEADLINES_MS.budgets,
            npmInvocation,
          }),
        ],
      },
    ],
    groups: [
      {
        label: "frontend",
        concurrency: 3,
        timeoutMs: VALIDATION_DEADLINES_MS.integrationFrontendLane,
        commands: frontend,
      },
      {
        label: "rust",
        concurrency: 1,
        timeoutMs: VALIDATION_DEADLINES_MS.integrationRustLane,
        cargoTargetDir: rustEnvironment.CARGO_TARGET_DIR,
        repositoryRoot: root,
        commands: rust,
      },
    ],
  };
}

async function main() {
  const options = parseIntegrationArguments(process.argv.slice(2));
  await runCliPlan(createIntegrationPlan(), options);
}

if (process.argv[1] && path.resolve(process.argv[1]) === scriptPath) {
  main().catch((error) => {
    process.stderr.write(`check-integration: ${error.message}\n`);
    process.exitCode = 1;
  });
}
