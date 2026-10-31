import { copyFileSync, mkdirSync, rmSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { INSTALLED_SMOKE_RUNNER_RELATIVE_PATHS } from "./beta0001-installed-smoke-source-closure.mjs";
import { buildSourceManifest } from "./generate-source-manifest.mjs";

const repositoryRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);

export function stageInstalledSmokeTestSource(testRoot) {
  const sourceRoot = path.join(testRoot, "authenticated-source");
  mkdirSync(sourceRoot);
  for (const relativePath of INSTALLED_SMOKE_RUNNER_RELATIVE_PATHS) {
    const destination = path.join(sourceRoot, ...relativePath.split("/"));
    mkdirSync(path.dirname(destination), { recursive: true });
    copyFileSync(path.join(repositoryRoot, relativePath), destination);
  }
  return {
    sourceRoot,
    sourceManifest: buildSourceManifest(sourceRoot, {
      files: INSTALLED_SMOKE_RUNNER_RELATIVE_PATHS,
      directories: [],
    }),
  };
}

// Windows releases a directory only after every handle on it closes, and the
// PowerShell, Job and guardian processes these tests start can still be
// closing theirs when a test ends. Retry EBUSY and friends with Node's linear
// backoff (100 ms, 200 ms, ...: 5.5 s over ten retries) before failing.
export function removeInstalledSmokeTestRoot(testRoot) {
  rmSync(testRoot, {
    recursive: true,
    force: true,
    maxRetries: 10,
    retryDelay: 100,
  });
}
