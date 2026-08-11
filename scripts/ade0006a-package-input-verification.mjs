import { packageBuildTreeBounds } from "./ade0006a-package-build-identity.mjs";
import { hashBoundedFile } from "./ade0006a-package-members.mjs";
import { observeAuthenticatedPackageTree } from "./ade0006a-package-tree.mjs";
import { verifyAuthenticatedPackageTreeSet } from "./ade0006a-package-tree-set.mjs";
import { stableJson } from "./ade0006a-c0b-common.mjs";

export async function verifyAuthenticatedPackageInputs({
  watchedTrees,
  watchedTreeSets,
  watchedFiles,
  frozen,
  maximumFileBytes,
}) {
  for (const [tree, bytesKey] of watchedTrees) {
    const observed = await observeAuthenticatedPackageTree({
      root: tree.root,
      bounds: packageBuildTreeBounds(frozen, bytesKey),
    });
    if (stableJson(observed.summary) !== stableJson(tree.summary)) {
      throw new Error(`authenticated-package-input-tree-changed:${bytesKey}`);
    }
  }
  for (const [treeSet, bytesKey] of watchedTreeSets) {
    await verifyAuthenticatedPackageTreeSet(
      treeSet,
      packageBuildTreeBounds(frozen, bytesKey),
    );
  }
  for (const file of watchedFiles) {
    if (
      (await hashBoundedFile(file.path, maximumFileBytes)).sha256 !==
      file.sha256
    ) {
      throw new Error("authenticated-package-input-file-changed");
    }
  }
}
