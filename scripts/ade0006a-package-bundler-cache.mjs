import { mkdir } from "node:fs/promises";
import path from "node:path";

import { stableJson } from "./ade0006a-c0b-common.mjs";
import { packageBuildTreeBounds } from "./ade0006a-package-build-identity.mjs";
import {
  copyAuthenticatedPackageTree,
  observeAuthenticatedPackageTree,
} from "./ade0006a-package-tree.mjs";

export function installAuthenticatedPackageBundlers({
  buildTools,
  contract,
  frozen,
  wixTree,
  nsisTree,
}) {
  Object.assign(buildTools, {
    prepareBundlers: async (target) => {
      const local = path.join(target, ".tauri");
      await mkdir(local, { recursive: false });
      for (const [kind, tree, bytesKey] of [
        ["wix", wixTree, "wixToolsBytes"],
        ["nsis", nsisTree, "nsisToolsBytes"],
      ]) {
        await copyAuthenticatedPackageTree({
          tree,
          destinationRoot: path.join(
            local,
            contract.bundlers[kind].directoryName,
          ),
          bounds: packageBuildTreeBounds(frozen, bytesKey),
        });
      }
    },
    verifyBundlers: async (target) => {
      for (const [kind, tree, bytesKey] of [
        ["wix", wixTree, "wixToolsBytes"],
        ["nsis", nsisTree, "nsisToolsBytes"],
      ]) {
        const observed = await observeAuthenticatedPackageTree({
          root: path.join(
            target,
            ".tauri",
            contract.bundlers[kind].directoryName,
          ),
          bounds: packageBuildTreeBounds(frozen, bytesKey),
        });
        if (stableJson(observed.summary) !== stableJson(tree.summary)) {
          throw new Error(`${kind}-bundler-tree-changed`);
        }
      }
    },
  });
}
