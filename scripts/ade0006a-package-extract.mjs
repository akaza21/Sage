import { spawn, spawnSync } from "node:child_process";
import { mkdir, realpath, stat, unlink } from "node:fs/promises";
import path from "node:path";

import { verifyTauriBundleExecutable } from "./ade0006a-package-binary.mjs";
import {
  hashBoundedFile,
  inspectPackageMembers,
} from "./ade0006a-package-members.mjs";

const extractorTimeoutMs = 2 * 60 * 1000;

function terminateTree(child) {
  const taskkill = path.join(
    process.env.SystemRoot ?? "C:\\Windows",
    "System32",
    "taskkill.exe",
  );
  const killed = spawnSync(taskkill, ["/PID", String(child.pid), "/T", "/F"], {
    windowsHide: true,
    stdio: "ignore",
    timeout: 5000,
  });
  if (killed.error || killed.status !== 0) child.kill();
}

function runBounded(executable, args, options) {
  return new Promise((resolve, reject) => {
    let stdoutBytes = 0;
    let stderrBytes = 0;
    let terminalReason;
    const child = spawn(executable, args, {
      cwd: options.cwd,
      env: options.env,
      windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
    });
    function capture(chunk, stream) {
      if (terminalReason) return;
      const maximum =
        stream === "stdout" ? options.stdoutMaximum : options.stderrMaximum;
      const next =
        (stream === "stdout" ? stdoutBytes : stderrBytes) + chunk.length;
      if (next > maximum) {
        terminalReason = `${stream}-oversize`;
        terminateTree(child);
      } else if (stream === "stdout") {
        stdoutBytes = next;
      } else {
        stderrBytes = next;
      }
    }
    child.stdout.on("data", (chunk) => capture(chunk, "stdout"));
    child.stderr.on("data", (chunk) => capture(chunk, "stderr"));
    child.on("error", (error) => {
      terminalReason ??= `spawn-${error.code ?? "failed"}`;
    });
    const timer = setTimeout(() => {
      terminalReason = "timeout";
      terminateTree(child);
    }, extractorTimeoutMs);
    child.on("close", (exitCode) => {
      clearTimeout(timer);
      if (terminalReason) {
        reject(new Error(`${options.label}-process-${terminalReason}`));
      } else if (exitCode !== 0) {
        reject(new Error(`${options.label}-process-exit-${exitCode}`));
      } else {
        resolve({ exitCode, stdoutBytes, stderrBytes });
      }
    });
  });
}

async function requiredAbsoluteFile(value, label) {
  if (!value || !path.isAbsolute(value)) {
    throw new Error(`${label}-path-invalid`);
  }
  const resolved = await realpath(value);
  if (!(await stat(resolved)).isFile()) {
    throw new Error(`${label}-path-invalid`);
  }
  return resolved;
}

async function fileEvidence(filePath, maximumBytes) {
  const evidence = await hashBoundedFile(filePath, maximumBytes);
  return { sha256: evidence.sha256, bytes: evidence.bytes };
}

export async function resolvePackageExtractors(options) {
  return {
    msi: await requiredAbsoluteFile(options.dark, "dark"),
    nsis: await requiredAbsoluteFile(options.sevenZip, "seven-zip"),
  };
}

export async function inspectPackageInstaller({
  kind,
  artifact,
  extractor,
  extractionRoot,
  memberRoot,
  participant,
  releaseExecutable,
  fixtureExecutable,
  frozen,
  sourceRoot,
  environment,
}) {
  if (!environment || Object.keys(environment).length < 1) {
    throw new Error("authenticated-extractor-environment-missing");
  }
  const bounds = frozen.package.observerBounds;
  const artifactMaximum =
    kind === "msi" ? bounds.msiArtifactBytes : bounds.nsisArtifactBytes;
  const artifactEvidence = await fileEvidence(artifact, artifactMaximum);
  const toolEvidence = await fileEvidence(
    extractor,
    bounds.executableArtifactBytes,
  );
  const output = path.join(extractionRoot, kind);
  await mkdir(output, { recursive: false });
  const msiMetadataPath =
    kind === "msi" ? path.join(extractionRoot, "msi-source.wxs") : undefined;
  const toolResult =
    kind === "msi"
      ? await runBounded(
          extractor,
          [artifact, "-nologo", "-x", output, "-o", msiMetadataPath],
          {
            cwd: sourceRoot,
            env: environment,
            label: "msi-extractor",
            stdoutMaximum: bounds.toolStdoutBytes,
            stderrMaximum: bounds.toolStderrBytes,
          },
        )
      : await runBounded(
          extractor,
          ["x", "-y", "-bb0", "-bso0", "-bsp0", `-o${output}`, artifact],
          {
            cwd: sourceRoot,
            env: environment,
            label: "nsis-extractor",
            stdoutMaximum: bounds.toolStdoutBytes,
            stderrMaximum: bounds.toolStderrBytes,
          },
        );
  let observed;
  let executableDerivation;
  let installedExecutablePath;
  try {
    observed = await inspectPackageMembers({
      participant,
      installer: kind,
      extractedRoot: output,
      outputDirectory: path.join(memberRoot, kind),
      fixtureExecutable,
      msiMetadataPath,
      frozen,
    });
    installedExecutablePath = path.join(
      output,
      ...observed.installedExecutable.extractedRelativePath.split("/"),
    );
    executableDerivation = await verifyTauriBundleExecutable({
      sourceExecutable: releaseExecutable,
      installedExecutable: installedExecutablePath,
      installer: kind,
      maximumBytes: bounds.executableArtifactBytes,
    });
  } finally {
    if (msiMetadataPath) await unlink(msiMetadataPath);
  }
  return {
    evidence: {
      kind,
      artifactSha256: artifactEvidence.sha256,
      artifactBytes: artifactEvidence.bytes,
      extractor: {
        tool: kind === "msi" ? "wix-dark" : "seven-zip",
        toolSha256: toolEvidence.sha256,
        ...toolResult,
      },
      executableDerivation,
      ...observed,
    },
    installedExecutablePath,
  };
}
