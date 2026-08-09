import { mkdir, rm } from "node:fs/promises";
import path from "node:path";

import { snapshotPackageArtifact } from "./ade0006a-package-snapshot.mjs";
import { observeAuthenticatedPackageTree } from "./ade0006a-package-tree.mjs";

function validateRelativePaths(relativePaths) {
  if (!Array.isArray(relativePaths) || relativePaths.length < 1) {
    throw new Error("authenticated package file set is invalid");
  }
  const unique = new Set();
  for (const relativePath of relativePaths) {
    if (
      typeof relativePath !== "string" ||
      path.posix.isAbsolute(relativePath) ||
      relativePath.includes("\\") ||
      relativePath
        .split("/")
        .some((segment) => !segment || segment === "." || segment === "..") ||
      unique.has(relativePath)
    ) {
      throw new Error("authenticated package file set member is invalid");
    }
    unique.add(relativePath);
  }
}

export async function stageAuthenticatedPackageFileSet({
  sourceRoot,
  destinationRoot,
  relativePaths,
  bounds,
  allowSourceHardLinks = false,
}) {
  validateRelativePaths(relativePaths);
  if (
    !path.isAbsolute(sourceRoot ?? "") ||
    !path.isAbsolute(destinationRoot ?? "") ||
    path.resolve(sourceRoot) === path.resolve(destinationRoot)
  ) {
    throw new Error("authenticated package file set paths are invalid");
  }
  let created = false;
  try {
    await mkdir(destinationRoot, { recursive: false });
    created = true;
    for (const relativePath of relativePaths) {
      const segments = relativePath.split("/");
      const destinationPath = path.join(destinationRoot, ...segments);
      await mkdir(path.dirname(destinationPath), { recursive: true });
      await snapshotPackageArtifact({
        sourcePath: path.join(sourceRoot, ...segments),
        destinationPath,
        maximumBytes: bounds.bytes,
        rejectHardLinks: !allowSourceHardLinks,
      });
    }
    const observed = await observeAuthenticatedPackageTree({
      root: destinationRoot,
      bounds,
    });
    if (observed.summary.fileCount !== relativePaths.length) {
      throw new Error("authenticated package file set does not reconcile");
    }
    return observed;
  } catch (error) {
    if (created) await rm(destinationRoot, { recursive: true, force: true });
    throw error;
  }
}
