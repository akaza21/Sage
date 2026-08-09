import { mkdir, rm } from "node:fs/promises";
import path from "node:path";

import { SHA256, stableJson } from "./ade0006a-c0b-common.mjs";
import { inspectBoundedTreeMembers } from "./ade0006a-package-members.mjs";
import { snapshotPackageArtifact } from "./ade0006a-package-snapshot.mjs";

function validateManifest(sourceManifest) {
  const bounds = sourceManifest?.include?.bounds;
  const files = sourceManifest?.files;
  if (
    !Array.isArray(files) ||
    !Number.isSafeInteger(bounds?.files) ||
    !Number.isSafeInteger(bounds?.fileBytes) ||
    !Number.isSafeInteger(bounds?.sourceBytes) ||
    !Number.isSafeInteger(bounds?.directoryDepth) ||
    files.length < 1 ||
    files.length !== sourceManifest?.fileCount ||
    files.length > bounds.files
  ) {
    throw new Error("source manifest shape or bounds are invalid");
  }
  const paths = new Set();
  let bytes = 0;
  for (const file of files) {
    if (
      typeof file?.path !== "string" ||
      path.posix.isAbsolute(file.path) ||
      file.path.includes("\\") ||
      file.path
        .split("/")
        .some((segment) => !segment || segment === "." || segment === "..") ||
      Buffer.byteLength(file.path) > 1024 ||
      paths.has(file.path) ||
      !Number.isSafeInteger(file.bytes) ||
      file.bytes < 1 ||
      file.bytes > bounds.fileBytes ||
      !SHA256.test(file.sha256 ?? "")
    ) {
      throw new Error("source manifest contains an invalid file identity");
    }
    paths.add(file.path);
    bytes += file.bytes;
  }
  if (
    bytes !== sourceManifest.totalBytes ||
    bytes > bounds.sourceBytes ||
    !Number.isSafeInteger(bytes)
  ) {
    throw new Error("source manifest bytes do not reconcile");
  }
  return { bounds, files, bytes };
}

function expectedRecords(files) {
  const directories = new Set();
  for (const file of files) {
    const segments = file.path.split("/");
    for (let index = 1; index < segments.length; index += 1) {
      directories.add(segments.slice(0, index).join("/"));
    }
  }
  return [
    ...[...directories].map((relativePath) => ({
      relativePath,
      type: "directory",
    })),
    ...files.map((file) => ({
      relativePath: file.path,
      type: "regularFile",
      bytes: file.bytes,
      sha256: file.sha256,
    })),
  ]
    .sort((left, right) =>
      Buffer.compare(
        Buffer.from(left.relativePath),
        Buffer.from(right.relativePath),
      ),
    )
    .map((record, index) => ({ ordinal: index + 1, ...record }));
}

export async function stagePackageSource({
  sourceRoot,
  sourceManifest,
  destinationRoot,
}) {
  if (
    !path.isAbsolute(sourceRoot) ||
    !path.isAbsolute(destinationRoot) ||
    path.resolve(sourceRoot) === path.resolve(destinationRoot)
  ) {
    throw new Error("source staging paths are invalid");
  }
  const { bounds, files, bytes } = validateManifest(sourceManifest);
  let created = false;
  try {
    await mkdir(destinationRoot, { recursive: false });
    created = true;
    for (const file of files) {
      const destinationPath = path.join(
        destinationRoot,
        ...file.path.split("/"),
      );
      await mkdir(path.dirname(destinationPath), { recursive: true });
      const observed = await snapshotPackageArtifact({
        sourcePath: path.join(sourceRoot, ...file.path.split("/")),
        destinationPath,
        maximumBytes: file.bytes,
      });
      if (observed.bytes !== file.bytes || observed.sha256 !== file.sha256) {
        throw new Error(`source manifest identity changed: ${file.path}`);
      }
    }
    const expected = expectedRecords(files);
    const observed = await inspectBoundedTreeMembers(destinationRoot, {
      membersPerInstaller: bounds.files * (bounds.directoryDepth + 1),
      extractedBytesPerInstaller: bounds.sourceBytes,
      pathUtf8Bytes: 1024,
    });
    if (stableJson(observed) !== stableJson(expected)) {
      throw new Error("staged source inventory does not match its manifest");
    }
    return { sourceRoot: destinationRoot, fileCount: files.length, bytes };
  } catch (error) {
    if (created) await rm(destinationRoot, { recursive: true, force: true });
    throw error;
  }
}
