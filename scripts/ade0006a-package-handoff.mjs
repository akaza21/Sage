import { realpath } from "node:fs/promises";
import path from "node:path";

import { hashBoundedFile } from "./ade0006a-package-members.mjs";

const participants = ["candidate", "control"];
const installers = ["msi", "nsis"];

function exactKeys(value, expected, label) {
  if (
    !value ||
    typeof value !== "object" ||
    Array.isArray(value) ||
    Object.keys(value).sort().join("\0") !== [...expected].sort().join("\0")
  ) {
    throw new Error(`artifact-handoff-${label}-shape-invalid`);
  }
}

function within(child, parent) {
  const relative = path.relative(parent, child);
  return (
    relative === "" ||
    (!relative.startsWith("..") && !path.isAbsolute(relative))
  );
}

async function authenticateFile({
  filePath,
  expected,
  label,
  maximumExecutableBytes,
  scratchRoot,
}) {
  if (typeof filePath !== "string" || !path.isAbsolute(filePath)) {
    throw new Error(`artifact-handoff-${label}-path-invalid`);
  }
  const resolved = await realpath(filePath);
  if (!within(resolved, scratchRoot)) {
    throw new Error(`artifact-handoff-${label}-outside-scratch`);
  }
  const evidence = await hashBoundedFile(resolved, maximumExecutableBytes, {
    rejectHardLinks: true,
  });
  if (
    evidence.sha256 !== expected?.sha256 ||
    evidence.bytes !== expected?.bytes
  ) {
    throw new Error(`artifact-handoff-${label}-mismatch`);
  }
  return { path: resolved, ...evidence };
}

export async function authenticatePackageArtifactHandoff({
  scratchRoot,
  artifacts,
  receipt,
  maximumExecutableBytes,
}) {
  const resolvedScratch = await realpath(scratchRoot);
  exactKeys(artifacts, participants, "participants");
  exactKeys(receipt?.participants, participants, "receipt-participants");
  const authenticated = {};
  for (const participant of participants) {
    const paths = artifacts[participant];
    const summary = receipt.participants[participant];
    exactKeys(
      paths,
      [
        "releaseExecutable",
        "fixtureExecutable",
        "characterizationExecutable",
        "installedExecutables",
      ],
      participant,
    );
    exactKeys(
      paths.installedExecutables,
      installers,
      `${participant}-installers`,
    );
    const releaseExpected = summary?.artifactSet?.entries?.find(
      ({ kind }) => kind === "executable",
    );
    const releaseExecutable = await authenticateFile({
      filePath: paths.releaseExecutable,
      expected: releaseExpected,
      label: `${participant}-release`,
      maximumExecutableBytes,
      scratchRoot: resolvedScratch,
    });
    const installedExecutables = {};
    for (const installer of installers) {
      const expected = summary?.installers?.find(
        ({ kind }) => kind === installer,
      )?.installedExecutable;
      installedExecutables[installer] = await authenticateFile({
        filePath: paths.installedExecutables[installer],
        expected,
        label: `${participant}-${installer}`,
        maximumExecutableBytes,
        scratchRoot: resolvedScratch,
      });
    }
    let fixtureExecutable = null;
    let characterizationExecutable = null;
    if (participant === "candidate") {
      fixtureExecutable = await authenticateFile({
        filePath: paths.fixtureExecutable,
        expected: {
          sha256: summary?.fixture?.executableSha256,
          bytes: summary?.fixture?.executableBytes,
        },
        label: "candidate-fixture",
        maximumExecutableBytes,
        scratchRoot: resolvedScratch,
      });
      characterizationExecutable = await authenticateFile({
        filePath: paths.characterizationExecutable,
        expected: {
          sha256: summary?.characterization?.executableSha256,
          bytes: summary?.characterization?.executableBytes,
        },
        label: "candidate-characterization",
        maximumExecutableBytes,
        scratchRoot: resolvedScratch,
      });
    } else if (
      paths.fixtureExecutable !== null ||
      paths.characterizationExecutable !== null
    ) {
      throw new Error("artifact-handoff-control-diagnostic-must-be-null");
    }
    authenticated[participant] = {
      releaseExecutable,
      fixtureExecutable,
      characterizationExecutable,
      installedExecutables,
    };
  }
  return authenticated;
}

export function assertPackageArtifactHandoffCallback(callback) {
  if (callback != null && typeof callback !== "function") {
    throw new Error("artifact-handoff-invalid");
  }
}

export async function deliverPackageArtifactHandoff({
  callback,
  receipt,
  receiptBytes,
  rawRoot,
  frozen,
  ...authentication
}) {
  assertPackageArtifactHandoffCallback(callback);
  if (!callback || receipt.result !== "passed") return false;
  const artifacts = await authenticatePackageArtifactHandoff({
    receipt,
    ...authentication,
  });
  await callback({
    receipt: structuredClone(receipt),
    receiptBytes: Buffer.from(receiptBytes),
    rawRoot,
    frozen: structuredClone(frozen),
    artifacts,
  });
  return true;
}
