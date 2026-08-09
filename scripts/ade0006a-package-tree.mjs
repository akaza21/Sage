import { mkdir, rm } from "node:fs/promises";
import path from "node:path";

import { recordDigest, stableJson } from "./ade0006a-c0b-common.mjs";
import { inspectBoundedTreeMembers } from "./ade0006a-package-members.mjs";
import { snapshotPackageArtifact } from "./ade0006a-package-snapshot.mjs";

function validateBounds(bounds) {
  if (
    !Number.isSafeInteger(bounds?.members) ||
    bounds.members < 1 ||
    !Number.isSafeInteger(bounds?.bytes) ||
    bounds.bytes < 1 ||
    !Number.isSafeInteger(bounds?.pathUtf8Bytes) ||
    bounds.pathUtf8Bytes < 1
  ) {
    throw new Error("authenticated package tree bounds are invalid");
  }
}

function summary(records) {
  const files = records.filter((record) => record.type === "regularFile");
  return {
    treeSha256: recordDigest(records),
    fileCount: files.length,
    bytes: files.reduce((total, file) => total + file.bytes, 0),
  };
}

export async function observeAuthenticatedPackageTree({ root, bounds }) {
  if (!path.isAbsolute(root)) {
    throw new Error("authenticated package tree root must be absolute");
  }
  validateBounds(bounds);
  const records = await inspectBoundedTreeMembers(root, {
    membersPerInstaller: bounds.members,
    extractedBytesPerInstaller: bounds.bytes,
    pathUtf8Bytes: bounds.pathUtf8Bytes,
  });
  if (records.length < 1) {
    throw new Error("authenticated package tree cannot be empty");
  }
  return { root, records, summary: summary(records) };
}

export async function copyAuthenticatedPackageTree({
  tree,
  destinationRoot,
  bounds,
}) {
  if (
    !path.isAbsolute(destinationRoot) ||
    !path.isAbsolute(tree?.root ?? "") ||
    path.resolve(destinationRoot) === path.resolve(tree.root)
  ) {
    throw new Error("authenticated package tree copy paths are invalid");
  }
  validateBounds(bounds);
  let created = false;
  try {
    await mkdir(destinationRoot, { recursive: false });
    created = true;
    for (const record of tree.records ?? []) {
      const destinationPath = path.join(
        destinationRoot,
        ...record.relativePath.split("/"),
      );
      if (record.type === "directory") {
        await mkdir(destinationPath, { recursive: true });
        continue;
      }
      await mkdir(path.dirname(destinationPath), { recursive: true });
      const observed = await snapshotPackageArtifact({
        sourcePath: path.join(tree.root, ...record.relativePath.split("/")),
        destinationPath,
        maximumBytes: record.bytes,
      });
      if (
        observed.bytes !== record.bytes ||
        observed.sha256 !== record.sha256
      ) {
        throw new Error(
          `authenticated package tree source changed: ${record.relativePath}`,
        );
      }
    }
    const copied = await observeAuthenticatedPackageTree({
      root: destinationRoot,
      bounds,
    });
    if (
      stableJson(copied.summary) !== stableJson(tree.summary) ||
      stableJson(copied.records) !== stableJson(tree.records)
    ) {
      throw new Error("authenticated package tree copy does not reconcile");
    }
    return copied;
  } catch (error) {
    if (created) await rm(destinationRoot, { recursive: true, force: true });
    throw error;
  }
}
