import { lstat, realpath, stat } from "node:fs/promises";
import path from "node:path";

import { runBounded } from "./ade0006a-package-build.mjs";
import { hashBoundedFile } from "./ade0006a-package-members.mjs";

const toolTimeoutMs = 20 * 60 * 1000;

export async function requiredPackageDirectory(value, label) {
  if (!path.isAbsolute(value ?? "")) throw new Error(`${label}-path-invalid`);
  const before = await lstat(value, { bigint: true });
  if (!before.isDirectory() || before.isSymbolicLink()) {
    throw new Error(`${label}-path-invalid`);
  }
  const resolved = await realpath(value);
  if (!(await stat(resolved)).isDirectory()) {
    throw new Error(`${label}-path-invalid`);
  }
  return resolved;
}

export async function packageToolFileIdentity(filePath, label, maximumBytes) {
  if (!path.isAbsolute(filePath ?? "")) {
    throw new Error(`${label}-path-invalid`);
  }
  const resolved = await realpath(filePath);
  const value = await hashBoundedFile(resolved, maximumBytes);
  return { path: resolved, sha256: value.sha256 };
}

export async function runPackageToolText(executable, args, options) {
  const result = await runBounded(executable, args, {
    cwd: options.cwd,
    env: options.env,
    timeoutMs: options.timeoutMs ?? toolTimeoutMs,
    label: options.label,
    stdoutMaximum: options.stdoutMaximum ?? 64 * 1024,
    stderrMaximum: options.stderrMaximum ?? 64 * 1024,
    captureStdout: true,
  });
  return result.stdout.toString("utf8").trim();
}

export function parsePackageRustVersion(value, label) {
  const lines = new Map(
    value
      .split(/\r?\n/u)
      .slice(1)
      .map((line) => line.split(/:\s*/u, 2)),
  );
  const first = value.split(/\r?\n/u, 1)[0];
  const release = lines.get("release");
  const commit = lines.get("commit-hash");
  const host = lines.get("host");
  if (!first || !release || !commit || !host) {
    throw new Error(`${label}-version-invalid`);
  }
  return { version: first, release, commit, host };
}
