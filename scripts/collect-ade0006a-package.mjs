import { mkdir, mkdtemp, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { sha256 } from "./ade0006a-c0b-common.mjs";
import { loadFrozenContracts } from "./ade0006a-c0b-contracts.mjs";
import { prepareAuthenticatedWindowsPackageBuild } from "./ade0006a-package-build-context.mjs";
import { resolvePackageExtractors } from "./ade0006a-package-extract.mjs";
import {
  assertPackageArtifactHandoffCallback,
  deliverPackageArtifactHandoff,
} from "./ade0006a-package-handoff.mjs";
import { observePackageParticipant } from "./ade0006a-package-observer.mjs";
import { runAuthenticatedInstalledSmoke } from "./beta0001-installed-smoke-handoff.mjs";
import { collectPackageSourceIdentity } from "./ade0006a-package-provenance.mjs";
import {
  derivePackageBudgetEvaluation,
  derivePackageDeltas,
  inspectPackageReceiptSanitized,
  validatePackageReceipt,
} from "./ade0006a-package-receipt.mjs";
import {
  classifyPackageFailure,
  failPackageCampaign,
  finalizePackageCampaign,
  packageAllocationEvidence,
  reservePackageCampaign,
} from "./ade0006a-package-retention.mjs";
import { buildSourceManifest } from "./generate-source-manifest.mjs";

const scriptPath = fileURLToPath(import.meta.url);
const participants = ["candidate", "control"];

function argument(name) {
  const index = process.argv.indexOf(name);
  return index < 0 ? undefined : process.argv[index + 1];
}

function validatedCampaignId(value) {
  if (!/^[a-z0-9][a-z0-9-]{0,63}$/.test(value ?? "")) {
    throw new Error("campaign-id-invalid");
  }
  return value;
}

function rawAccounting(sourceManifestBytes, observed) {
  const shards = participants.flatMap((participant) =>
    observed[participant].installers.flatMap(
      (installer) => installer.memberManifest.shards,
    ),
  );
  return {
    rawFiles: 1 + shards.length,
    rawBytes:
      sourceManifestBytes +
      shards.reduce((total, shard) => total + shard.bytes, 0),
  };
}

function serializeWithExactCuratedBytes(receipt) {
  let bytes = 0;
  for (let attempt = 0; attempt < 8; attempt += 1) {
    receipt.retention.curatedBytes = bytes;
    const serialized = Buffer.from(JSON.stringify(receipt));
    if (serialized.byteLength === bytes) return serialized;
    bytes = serialized.byteLength;
  }
  throw new Error("curated-byte-fixed-point-did-not-converge");
}

export async function collectPackage(options) {
  assertPackageArtifactHandoffCallback(options.onAuthenticatedArtifacts);
  if (process.platform !== "win32") throw new Error("windows-required");
  const campaignId = validatedCampaignId(options.campaignId);
  const sourceRoot = await realpath(options.sourceRoot);
  const frozen = loadFrozenContracts(sourceRoot);
  const startedAtUtc = new Date().toISOString();
  const sourceBefore = buildSourceManifest(sourceRoot);
  const sourceManifestBytes = Buffer.from(JSON.stringify(sourceBefore));
  let packageFinalized = false;
  let campaignReserved = false;
  let allocationIdentity;
  let rawRoot;
  const scratchRoot = await mkdtemp(path.join(tmpdir(), "sage-package-pair-"));
  try {
    const buildContext = await prepareAuthenticatedWindowsPackageBuild({
      sourceRoot,
      sourceManifest: sourceBefore,
      scratchRoot,
      frozen,
      options: {
        rustToolchainRoot: options.rustToolchainRoot,
        cargoHome: options.cargoHome,
        node: options.node,
        npmPackageRoot: options.npmPackageRoot,
        gitRoot: options.gitRoot,
        wixToolsRoot: options.wixToolsRoot,
        nsisToolsRoot: options.nsisToolsRoot,
        msvcToolsRoot: options.msvcToolsRoot,
        windowsSdkRoot: options.windowsSdkRoot,
        windowsSdkVersion: options.windowsSdkVersion,
        systemRoot: process.env.SystemRoot,
        comSpec: process.env.ComSpec,
      },
    });
    const buildTools = buildContext.buildTools;
    const extractors = await resolvePackageExtractors({
      dark: path.join(options.wixToolsRoot, "dark.exe"),
      sevenZip: options.sevenZip,
    });
    const sourceIdentity = await collectPackageSourceIdentity({
      sourceRoot,
      buildTools,
    });
    ({ allocationIdentity } = packageAllocationEvidence({
      sourceRoot: buildContext.sourceRoot,
      sourceManifest: sourceBefore,
      sourceIdentity,
      buildIdentity: buildContext.buildIdentity,
      extractors,
      frozen,
    }));
    ({ rawRoot } = await reservePackageCampaign({
      sourceRoot,
      campaignId,
      allocationIdentity,
      sourceAggregateSha256: sourceBefore.aggregateSha256,
      startedAtUtc,
      frozen,
    }));
    campaignReserved = true;
    await mkdir(path.join(rawRoot, "package"), { recursive: true });
    for (const participant of participants) {
      await mkdir(path.join(rawRoot, "package", participant));
    }
    const sourceManifestPath = path.join(rawRoot, "source-manifest-v1.json");
    await writeFile(sourceManifestPath, sourceManifestBytes, { flag: "wx" });
    const observed = {};
    const ephemeralArtifacts = {};
    for (const participant of participants) {
      const participantScratch = path.join(scratchRoot, participant);
      await mkdir(participantScratch);
      const observation = await observePackageParticipant({
        sourceRoot: buildContext.sourceRoot,
        participant,
        rawRoot,
        scratchRoot: participantScratch,
        buildTools,
        extractors,
        frozen,
      });
      observed[participant] = observation.summary;
      ephemeralArtifacts[participant] = observation.ephemeral;
    }
    const sourceAfter = buildSourceManifest(sourceRoot);
    if (
      sourceAfter.aggregateSha256 !== sourceBefore.aggregateSha256 ||
      sourceAfter.fileCount !== sourceBefore.fileCount ||
      sourceAfter.totalBytes !== sourceBefore.totalBytes
    ) {
      throw new Error("source-changed-during-package-campaign");
    }
    await buildContext.verifyInputs();
    const deltas = derivePackageDeltas(
      observed.candidate,
      observed.control,
      frozen.frontendBaseline,
    );
    const budgetEvaluation = derivePackageBudgetEvaluation(deltas, frozen);
    const result = budgetEvaluation.every(({ passed }) => passed)
      ? "passed"
      : "failed";
    const completedAtUtc = new Date().toISOString();
    const receipt = {
      schemaVersion: 1,
      workItem: "ADE-0006A",
      phase: "Package",
      campaignId,
      allocationIdentity,
      startedAtUtc,
      completedAtUtc,
      result,
      contractManifestSha256: frozen.contractManifestSha256,
      packageContractSha256: frozen.packageContractSha256,
      buildIdentity: buildContext.buildIdentity,
      platform: "windows",
      architecture: "x86_64",
      source: {
        commit: sourceIdentity.commit,
        tree: sourceIdentity.tree,
        aggregateSha256: sourceBefore.aggregateSha256,
        manifestSha256: sha256(sourceManifestBytes),
        fileCount: sourceBefore.fileCount,
        bytes: sourceBefore.totalBytes,
      },
      frontendBaseline: frozen.frontendBaseline,
      participants: observed,
      deltas,
      budgetEvaluation,
      webviewProvisioning: {
        mode: frozen.package.webviewProvisioning.mode,
        whenRuntimeAbsent: frozen.package.webviewProvisioning.whenRuntimeAbsent,
        networkRequiredWhenTriggered:
          frozen.package.webviewProvisioning.networkRequiredWhenTriggered,
      },
      retention: {
        rawFiles: 0,
        rawBytes: 0,
        curatedFiles: 1,
        curatedBytes: 0,
      },
    };
    Object.assign(
      receipt.retention,
      rawAccounting(sourceManifestBytes.byteLength, observed),
    );
    const errors = [];
    inspectPackageReceiptSanitized(receipt, errors);
    if (errors.length > 0) throw new Error(errors.join(","));
    const receiptBytes = serializeWithExactCuratedBytes(receipt);
    const validationErrors = await validatePackageReceipt({
      receipt,
      receiptBytes,
      rawRoot,
      frozen,
    });
    if (validationErrors.length > 0) {
      throw new Error(validationErrors.join(","));
    }
    await finalizePackageCampaign({
      sourceRoot,
      campaignId,
      allocationIdentity,
      completedAtUtc,
      receipt,
      receiptBytes,
      frozen,
    });
    packageFinalized = true;
    await deliverPackageArtifactHandoff({
      callback: options.onAuthenticatedArtifacts,
      scratchRoot,
      artifacts: ephemeralArtifacts,
      receipt,
      receiptBytes,
      rawRoot,
      frozen,
      maximumExecutableBytes:
        frozen.package.observerBounds.executableArtifactBytes,
      git: buildTools.git,
    });
    return { receipt, receiptBytes };
  } catch (error) {
    if (campaignReserved && !packageFinalized) {
      const failureCode = classifyPackageFailure(error);
      try {
        await failPackageCampaign({
          sourceRoot,
          campaignId,
          allocationIdentity,
          completedAtUtc: new Date().toISOString(),
          failureCode,
          frozen,
        });
      } catch (settlementError) {
        throw new Error("ledger-reconciliation-failed", {
          cause: settlementError,
        });
      }
      throw new Error(`package-campaign-failed:${failureCode}`, {
        cause: error,
      });
    }
    if (packageFinalized) {
      throw new Error("package-artifact-handoff-failed", { cause: error });
    }
    throw error;
  } finally {
    await rm(scratchRoot, { recursive: true, force: true });
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === scriptPath) {
  try {
    const sourceRoot = argument("--source-root");
    const campaignId = argument("--campaign-id");
    const node = argument("--node");
    const gitRoot = argument("--git-root");
    const smokeOutput = argument("--installed-smoke-output");
    const smokeSignerThumbprint = argument(
      "--installed-smoke-signer-thumbprint",
    );
    if (smokeOutput && !/^[0-9a-f]{40}$/u.test(smokeSignerThumbprint ?? "")) {
      throw new Error("installed-smoke-signer-thumbprint-invalid");
    }
    let installedSmokeReceipt = null;
    const { receipt, receiptBytes } = await collectPackage({
      campaignId,
      sourceRoot,
      rustToolchainRoot: argument("--rust-toolchain-root"),
      cargoHome: argument("--cargo-home"),
      node,
      npmPackageRoot: argument("--npm-package-root"),
      gitRoot,
      wixToolsRoot: argument("--wix-tools-root"),
      nsisToolsRoot: argument("--nsis-tools-root"),
      msvcToolsRoot: argument("--msvc-tools-root"),
      windowsSdkRoot: argument("--windows-sdk-root"),
      windowsSdkVersion: argument("--windows-sdk-version"),
      sevenZip: argument("--seven-zip"),
      onAuthenticatedArtifacts: smokeOutput
        ? async (handoff) => {
            installedSmokeReceipt = await runAuthenticatedInstalledSmoke({
              handoff,
              campaignId,
              outputDirectory: smokeOutput,
              powershell: path.join(
                process.env.SystemRoot ?? "C:\\Windows",
                "System32",
                "WindowsPowerShell",
                "v1.0",
                "powershell.exe",
              ),
              node,
              expectedSignerThumbprint: smokeSignerThumbprint,
              sourceRoot,
            });
          }
        : undefined,
    });
    process.stdout.write(
      JSON.stringify({
        status: receipt.result,
        campaignId: receipt.campaignId,
        receiptSha256: sha256(receiptBytes),
        failedBudgets: receipt.budgetEvaluation
          .filter(({ passed }) => !passed)
          .map(({ metric }) => metric),
        ...(smokeOutput
          ? {
              installedSmoke: installedSmokeReceipt
                ? {
                    result: installedSmokeReceipt.result,
                    failure: installedSmokeReceipt.failure,
                  }
                : null,
            }
          : {}),
      }),
    );
    if (receipt.result !== "passed") process.exitCode = 2;
    if (installedSmokeReceipt && installedSmokeReceipt.result !== "passed") {
      process.exitCode = 3;
    }
  } catch (error) {
    process.stderr.write(`package-observer: ${error.message}\n`);
    process.exitCode = 1;
  }
}
