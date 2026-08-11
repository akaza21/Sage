import { recordDigest } from "./ade0006a-c0b-common.mjs";

export function packageBuildTreeBounds(frozen, bytesKey) {
  const bounds = frozen.package.authenticatedBuild.bounds;
  return {
    members: bounds.treeMembers,
    bytes: bounds[bytesKey],
    pathUtf8Bytes: bounds.pathUtf8Bytes,
  };
}

function requiredTreeFile(tree, relativePath, label) {
  const record = tree.records.find(
    (value) =>
      value.type === "regularFile" && value.relativePath === relativePath,
  );
  if (!record) throw new Error(`${label}-missing`);
  return record.sha256;
}

export function deriveAuthenticatedPackageBuildIdentity({
  contract,
  versions,
  buildTools,
  node,
  powershell,
  npmCli,
  git,
  launchers,
  msvc,
  sdk,
  trees,
  windowsSdkVersion,
}) {
  const buildIdentity = {
    schemaVersion: 1,
    recipe: contract.recipe,
    sourceStage: contract.sourceStage,
    environmentPolicySha256: recordDigest(contract.environment),
    rustToolchain: {
      directoryName: contract.rust.toolchainDirectoryName,
      release: versions.rustc.release,
      host: versions.rustc.host,
      cargoVersion: versions.cargo.release,
      cargoCommit: versions.cargo.commit,
      cargoExecutableSha256: buildTools.cargo.sha256,
      rustcVersion: versions.rustc.release,
      rustcCommit: versions.rustc.commit,
      rustcExecutableSha256: buildTools.rustc.sha256,
      ...trees.rust.summary,
    },
    cargoRegistry: trees.cargoRegistry.summary,
    node: {
      version: versions.node,
      executableSha256: node.sha256,
    },
    powershell: {
      executableSha256: powershell.sha256,
    },
    npm: {
      version: versions.npm,
      cliSha256: npmCli.sha256,
      ...trees.npm.summary,
    },
    nodeModules: trees.nodeModules.summary,
    launchers: {
      typescriptSha256: launchers.typescript.sha256,
      viteSha256: launchers.vite.sha256,
      tauriJsSha256: buildTools["tauri-cli-js"].sha256,
      tauriNativeSha256: buildTools["tauri-cli-native"].sha256,
    },
    git: {
      version: versions.git,
      executableSha256: git.sha256,
      ...trees.gitRuntime.summary,
    },
    msvc: {
      version: contract.nativeTools.msvc.directoryName,
      clSha256: msvc.cl.sha256,
      linkSha256: msvc.link.sha256,
      ...trees.msvcInputs.summary,
    },
    windowsSdk: {
      version: windowsSdkVersion,
      rcSha256: sdk.rc.sha256,
      mtSha256: sdk.mt.sha256,
      ...trees.windowsSdkInputs.summary,
    },
    bundlers: {
      wix: {
        ...trees.wix.summary,
        candleSha256: requiredTreeFile(trees.wix, "candle.exe", "wix-candle"),
        lightSha256: requiredTreeFile(trees.wix, "light.exe", "wix-light"),
        darkSha256: requiredTreeFile(trees.wix, "dark.exe", "wix-dark"),
      },
      nsis: {
        ...trees.nsis.summary,
        launcherSha256: requiredTreeFile(
          trees.nsis,
          "makensis.exe",
          "nsis-launcher",
        ),
        compilerSha256: requiredTreeFile(
          trees.nsis,
          "Bin/makensis.exe",
          "nsis-compiler",
        ),
      },
    },
  };
  return {
    buildIdentity,
    buildIdentitySha256: recordDigest(buildIdentity),
  };
}
