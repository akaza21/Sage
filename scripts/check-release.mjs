import path from "node:path";
import { fileURLToPath } from "node:url";

import { buildRustEnvironment, runCliPlan } from "./lib/check-process.mjs";
import {
  boundedCommand,
  COMMAND_DEADLINES_MS,
  createRustValidationCommands,
  npmCommand,
  resolveNpmInvocation,
  VALIDATION_DEADLINES_MS,
} from "./lib/check-plans.mjs";

const scriptPath = fileURLToPath(import.meta.url);
const repositoryRoot = path.resolve(path.dirname(scriptPath), "..");
const REQUIRED_PACKAGE_ARGUMENTS = [
  "--source-root",
  "--campaign-id",
  "--rust-toolchain-root",
  "--cargo-home",
  "--node",
  "--npm-package-root",
  "--git-root",
  "--wix-tools-root",
  "--nsis-tools-root",
  "--msvc-tools-root",
  "--windows-sdk-root",
  "--windows-sdk-version",
  "--seven-zip",
];
const OPTIONAL_PACKAGE_ARGUMENTS = [
  "--installed-smoke-output",
  "--installed-smoke-signer-thumbprint",
];

function validatedPackageArguments(arguments_) {
  const allowed = new Set([
    ...REQUIRED_PACKAGE_ARGUMENTS,
    ...OPTIONAL_PACKAGE_ARGUMENTS,
  ]);
  const seen = new Set();
  for (let index = 0; index < arguments_.length; index += 2) {
    const name = arguments_[index];
    const value = arguments_[index + 1];
    if (!allowed.has(name))
      throw new Error(`unknown package argument: ${name ?? "<missing>"}`);
    if (seen.has(name)) throw new Error(`duplicate package argument: ${name}`);
    if (!value || value.startsWith("--"))
      throw new Error(`package argument has no value: ${name}`);
    seen.add(name);
  }
  const missing = REQUIRED_PACKAGE_ARGUMENTS.filter((name) => !seen.has(name));
  if (missing.length > 0) {
    throw new Error(
      `missing required package arguments: ${missing.join(", ")}`,
    );
  }
  const smokeCount = OPTIONAL_PACKAGE_ARGUMENTS.filter((name) =>
    seen.has(name),
  ).length;
  if (smokeCount === 1)
    throw new Error("installed smoke requires output and signer thumbprint");
  const valueFor = (name) => arguments_[arguments_.indexOf(name) + 1];
  if (!/^[a-z0-9][a-z0-9-]{0,63}$/u.test(valueFor("--campaign-id"))) {
    throw new Error("package campaign id is invalid");
  }
  if (
    seen.has("--installed-smoke-signer-thumbprint") &&
    !/^[0-9a-f]{40}$/u.test(valueFor("--installed-smoke-signer-thumbprint"))
  ) {
    throw new Error("installed smoke signer thumbprint is invalid");
  }
  return arguments_;
}

export function parseReleaseArguments(arguments_) {
  const separator = arguments_.indexOf("--");
  const wrapperArguments =
    separator < 0 ? arguments_ : arguments_.slice(0, separator);
  const packageArguments = separator < 0 ? [] : arguments_.slice(separator + 1);
  let mode;
  let dryRun = false;
  for (const argument of wrapperArguments) {
    if (argument === "--dry-run") dryRun = true;
    else if (
      argument === "--validation-only" ||
      argument === "--package-campaign"
    ) {
      if (mode)
        throw new Error("release check accepts exactly one explicit mode");
      mode = argument.slice(2);
    } else throw new Error(`unknown release-check argument: ${argument}`);
  }
  if (!mode) throw new Error("release check requires an explicit mode");
  if (mode === "validation-only" && packageArguments.length > 0) {
    throw new Error("validation-only mode does not accept package arguments");
  }
  if (mode === "package-campaign") validatedPackageArguments(packageArguments);
  return { dryRun, mode, packageArguments };
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
  if (options.mode === "package-campaign") {
    commands.push(
      boundedCommand(
        {
          label: "release:package-and-installed-smoke-handoff",
          command: process.execPath,
          args: [
            "scripts/collect-ade0006a-package.mjs",
            ...options.packageArguments,
          ],
          cwd: root,
        },
        COMMAND_DEADLINES_MS.packageCampaign,
      ),
    );
  }
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
