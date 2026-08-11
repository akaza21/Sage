import assert from "node:assert/strict";
import { createHash, createHmac } from "node:crypto";
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { runBounded } from "./ade0006a-package-build.mjs";
import {
  INSTALLED_SMOKE_LOCKED_BOOTSTRAP_MAXIMUM_BYTES,
  INSTALLED_SMOKE_LOCKED_LOADER_ENCODED_COMMAND,
  lockedPowerShellArguments,
  serializeLockedPowerShellRequest,
} from "./beta0001-windows-locked-loader.mjs";

function identity(root, relativePath) {
  const bytes = readFileSync(path.join(root, ...relativePath.split("/")));
  return {
    relativePath,
    sha256: createHash("sha256").update(bytes).digest("hex"),
    bytes: bytes.byteLength,
  };
}

function powershellPath() {
  return path.join(
    process.env.SystemRoot ?? "C:\\Windows",
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
}

async function runLocked(root, request, label) {
  return runBounded(powershellPath(), lockedPowerShellArguments(), {
    cwd: root,
    env: process.env,
    timeoutMs: 10_000,
    label,
    stdoutMaximum: 16 * 1024,
    stderrMaximum: 16 * 1024,
    captureStdout: true,
    stdin: request,
    stdinMaximum: INSTALLED_SMOKE_LOCKED_BOOTSTRAP_MAXIMUM_BYTES,
  });
}

test(
  "locked loader rejects mutation after parent authentication and retains deny-write handles",
  { skip: process.platform !== "win32" },
  async () => {
    const root = mkdtempSync(path.join(tmpdir(), "sage-locked-loader-"));
    try {
      const scripts = path.join(root, "scripts");
      mkdirSync(scripts);
      const entry = "scripts/entry.ps1";
      const locked = "scripts/locked.psm1";
      writeFileSync(path.join(root, locked), "locked-content");
      writeFileSync(
        path.join(root, entry),
        String.raw`param([string]$AuthenticatedSourceRoot)
$target = Join-Path $AuthenticatedSourceRoot "scripts/locked.psm1"
try { [IO.File]::WriteAllText($target, "mutated"); throw "write unexpectedly succeeded" }
catch [IO.IOException] { [Console]::Out.Write("locked") }
`,
      );
      const identities = [identity(root, entry), identity(root, locked)];
      const request = serializeLockedPowerShellRequest({
        sourceRoot: root,
        entryRelativePath: entry,
        identities,
        parameters: { AuthenticatedSourceRoot: root },
      });
      const result = await runLocked(root, request, "locked-loader-hold");
      assert.equal(result.stdout.toString("utf8"), "locked");
      assert.equal(
        readFileSync(path.join(root, locked), "utf8"),
        "locked-content",
      );
      writeFileSync(path.join(root, locked), "released-after-exit");

      const changedRequest = serializeLockedPowerShellRequest({
        sourceRoot: root,
        entryRelativePath: entry,
        identities: [identity(root, entry), identity(root, locked)],
        parameters: { AuthenticatedSourceRoot: root },
      });
      writeFileSync(path.join(root, locked), "mutation-after-authentication");
      await assert.rejects(
        runLocked(root, changedRequest, "locked-loader-changed"),
        /locked-loader-changed-process-exit-/,
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  },
);

test(
  "outer controller forcibly converges its real native Job before reporting timeout",
  { skip: process.platform !== "win32" },
  async () => {
    const root = mkdtempSync(path.join(tmpdir(), "sage-outer-job-"));
    try {
      const relativePaths = [
        "scripts/control-windows-installed-smoke.ps1",
        "scripts/footprint/WindowsJob.psm1",
        "scripts/footprint/WindowsJobCompletion.psm1",
        "scripts/dummy-campaign.ps1",
      ];
      for (const relativePath of relativePaths.slice(0, 3)) {
        const destination = path.join(root, ...relativePath.split("/"));
        mkdirSync(path.dirname(destination), { recursive: true });
        copyFileSync(path.resolve(relativePath), destination);
      }
      mkdirSync(path.join(root, "scripts"), { recursive: true });
      writeFileSync(
        path.join(root, "scripts", "dummy-campaign.ps1"),
        "Start-Sleep -Seconds 30\n",
      );
      const identities = relativePaths.map((value) => identity(root, value));
      const campaignId = "outer-job-proof-01";
      const campaign = serializeLockedPowerShellRequest({
        sourceRoot: root,
        entryRelativePath: "scripts/dummy-campaign.ps1",
        identities,
        parameters: {},
      });
      const powershell = powershellPath();
      const powerIdentity = identity(
        path.dirname(powershell),
        path.basename(powershell),
      );
      const controller = serializeLockedPowerShellRequest({
        sourceRoot: root,
        entryRelativePath: "scripts/control-windows-installed-smoke.ps1",
        identities,
        parameters: {
          CampaignId: campaignId,
          AuthenticatedSourceRoot: root,
          PowerShellPath: powershell,
          PowerShellSha256: powerIdentity.sha256,
          PowerShellBytes: powerIdentity.bytes,
          CampaignBootstrapBase64: campaign.toString("base64"),
          LockedLoaderEncodedCommand:
            INSTALLED_SMOKE_LOCKED_LOADER_ENCODED_COMMAND,
          OuterJobTimeoutMilliseconds: 200,
        },
      });
      const result = await runLocked(root, controller, "outer-job-controller");
      const proof = JSON.parse(result.stdout.toString("utf8"));
      assert.equal(proof.status, "timeout");
      assert.equal(proof.jobConverged, true);
      assert.equal(proof.forced, true);
      assert.equal(proof.orphanProcessesAfterClose, 0);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  },
);

test(
  "outer controller rejects receipt bytes replaced after campaign authentication",
  { skip: process.platform !== "win32" },
  async () => {
    const root = mkdtempSync(path.join(tmpdir(), "sage-outer-receipt-origin-"));
    try {
      const relativePaths = [
        "scripts/control-windows-installed-smoke.ps1",
        "scripts/footprint/WindowsJob.psm1",
        "scripts/footprint/WindowsJobCompletion.psm1",
        "scripts/dummy-campaign.ps1",
      ];
      for (const relativePath of relativePaths.slice(0, 3)) {
        const destination = path.join(root, ...relativePath.split("/"));
        mkdirSync(path.dirname(destination), { recursive: true });
        copyFileSync(path.resolve(relativePath), destination);
      }
      const output = path.join(root, "output");
      mkdirSync(output);
      const campaignId = "outer-receipt-origin-01";
      const key = Buffer.alloc(32, 0x6b);
      const original = Buffer.from('{"result":"failed"}\n');
      const replacement = Buffer.from('{"result":"passed"}\n');
      const authentication = Buffer.from(
        `${JSON.stringify({
          schemaVersion: 1,
          campaignId,
          receiptSha256: createHash("sha256").update(original).digest("hex"),
          authenticationSha256: createHmac("sha256", key)
            .update(original)
            .digest("hex"),
        })}\n`,
      );
      writeFileSync(
        path.join(root, "scripts", "dummy-campaign.ps1"),
        `param([string]$OutputDirectory, [string]$CleanupKeyBase64)\n` +
          `[IO.File]::WriteAllBytes((Join-Path $OutputDirectory "receipt.json"), [Convert]::FromBase64String("${original.toString("base64")}"))\n` +
          `[IO.File]::WriteAllBytes((Join-Path $OutputDirectory "receipt-authentication.json"), [Convert]::FromBase64String("${authentication.toString("base64")}"))\n` +
          `[IO.File]::WriteAllBytes((Join-Path $OutputDirectory "receipt.json"), [Convert]::FromBase64String("${replacement.toString("base64")}"))\n`,
      );
      const identities = relativePaths.map((value) => identity(root, value));
      const campaign = serializeLockedPowerShellRequest({
        sourceRoot: root,
        entryRelativePath: "scripts/dummy-campaign.ps1",
        identities,
        parameters: {
          OutputDirectory: output,
          CleanupKeyBase64: key.toString("base64"),
        },
      });
      const powershell = powershellPath();
      const powerIdentity = identity(
        path.dirname(powershell),
        path.basename(powershell),
      );
      const controller = serializeLockedPowerShellRequest({
        sourceRoot: root,
        entryRelativePath: "scripts/control-windows-installed-smoke.ps1",
        identities,
        parameters: {
          CampaignId: campaignId,
          AuthenticatedSourceRoot: root,
          PowerShellPath: powershell,
          PowerShellSha256: powerIdentity.sha256,
          PowerShellBytes: powerIdentity.bytes,
          CampaignBootstrapBase64: campaign.toString("base64"),
          LockedLoaderEncodedCommand:
            INSTALLED_SMOKE_LOCKED_LOADER_ENCODED_COMMAND,
          OuterJobTimeoutMilliseconds: 10_000,
        },
      });
      const result = await runLocked(root, controller, "outer-receipt-origin");
      const proof = JSON.parse(result.stdout.toString("utf8"));
      assert.equal(proof.status, "completed");
      assert.equal(proof.jobConverged, true);
      assert.equal(proof.receiptSha256, null);
      assert.equal(proof.receiptAuthenticationSha256, null);
      assert.equal(
        readFileSync(path.join(output, "receipt.json"), "utf8"),
        replacement.toString("utf8"),
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  },
);
