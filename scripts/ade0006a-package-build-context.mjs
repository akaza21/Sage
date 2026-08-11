import path from "node:path";

import {
  resolvePackageBuildTools,
  runBounded,
} from "./ade0006a-package-build.mjs";
import { installAuthenticatedPackageBundlers } from "./ade0006a-package-bundler-cache.mjs";
import {
  deriveAuthenticatedPackageBuildIdentity,
  packageBuildTreeBounds,
} from "./ade0006a-package-build-identity.mjs";
import { prepareIsolatedPackageBuildHome } from "./ade0006a-package-build-home.mjs";
import {
  createPackageCargoEnvironment,
  createPackageGitEnvironment,
  createPackageNpmEnvironment,
} from "./ade0006a-package-environment.mjs";
import { verifyAuthenticatedPackageInputs } from "./ade0006a-package-input-verification.mjs";
import { stageAuthenticatedPackageFileSet } from "./ade0006a-package-file-set.mjs";
import { stagePackageSource } from "./ade0006a-package-source-stage.mjs";
import { observeAuthenticatedPackageTree } from "./ade0006a-package-tree.mjs";
import {
  observeAuthenticatedPackageTreeSet,
  resolveAuthenticatedPackageTreeSetMembers,
} from "./ade0006a-package-tree-set.mjs";
import {
  packageToolFileIdentity,
  parsePackageRustVersion,
  requiredPackageDirectory,
  runPackageToolText,
} from "./ade0006a-package-tool-authentication.mjs";
import { buildSourceManifest } from "./generate-source-manifest.mjs";

const toolTimeoutMs = 20 * 60 * 1000;

export async function prepareAuthenticatedWindowsPackageBuild({
  sourceRoot,
  sourceManifest,
  scratchRoot,
  frozen,
  options,
}) {
  const contract = frozen.package.authenticatedBuild;
  const staged = await stagePackageSource({
    sourceRoot,
    sourceManifest,
    destinationRoot: path.join(scratchRoot, "source"),
  });
  const cargoHome = await requiredPackageDirectory(
    options.cargoHome,
    "cargo-home",
  );
  const directories = await prepareIsolatedPackageBuildHome({
    root: path.join(scratchRoot, "build"),
    cargoHome,
  });
  const rustToolchainRoot = await requiredPackageDirectory(
    options.rustToolchainRoot,
    "rust-toolchain-root",
  );
  const npmPackageRoot = await requiredPackageDirectory(
    options.npmPackageRoot,
    "npm-root",
  );
  const wixToolsRoot = await requiredPackageDirectory(
    options.wixToolsRoot,
    "wix-root",
  );
  const nsisToolsRoot = await requiredPackageDirectory(
    options.nsisToolsRoot,
    "nsis-root",
  );
  const msvcToolsRoot = await requiredPackageDirectory(
    options.msvcToolsRoot,
    "msvc-root",
  );
  const windowsSdkRoot = await requiredPackageDirectory(
    options.windowsSdkRoot,
    "sdk-root",
  );
  const gitRoot = await requiredPackageDirectory(options.gitRoot, "git-root");
  const nativeContract = contract.nativeTools;
  if (
    path.basename(msvcToolsRoot) !== nativeContract.msvc.directoryName ||
    options.windowsSdkVersion !== nativeContract.windowsSdk.version ||
    path.basename(gitRoot).toLowerCase() !== contract.git.directoryName
  ) {
    throw new Error("authenticated-package-native-version-mismatch");
  }
  const maximum = frozen.package.observerBounds.executableArtifactBytes;
  const node = await packageToolFileIdentity(options.node, "node", maximum);
  const powershell = await packageToolFileIdentity(
    path.join(
      options.systemRoot,
      "System32",
      "WindowsPowerShell",
      "v1.0",
      "powershell.exe",
    ),
    "powershell",
    maximum,
  );
  const gitRuntimeTree = await stageAuthenticatedPackageFileSet({
    sourceRoot: path.join(
      gitRoot,
      ...contract.git.sourceRuntimeRoot.split("/"),
    ),
    destinationRoot: path.join(scratchRoot, "git-runtime"),
    relativePaths: contract.git.runtimeFiles,
    bounds: packageBuildTreeBounds(frozen, "gitRuntimeBytes"),
    allowSourceHardLinks: true,
  });
  const git = await packageToolFileIdentity(
    path.join(gitRuntimeTree.root, ...contract.git.executable.split("/")),
    "git",
    maximum,
  );
  const environmentContext = {
    systemRoot: options.systemRoot,
    comSpec: options.comSpec,
    ...directories,
    cargoHome,
    cargo: path.join(rustToolchainRoot, "bin", "cargo.exe"),
    rustc: path.join(rustToolchainRoot, "bin", "rustc.exe"),
    node: node.path,
    git: git.path,
    msvcToolsRoot,
    windowsSdkRoot,
    windowsSdkVersion: options.windowsSdkVersion,
  };
  const npmEnvironment = createPackageNpmEnvironment(environmentContext);
  const cargoEnvironment = createPackageCargoEnvironment(
    environmentContext,
    directories.metadataTarget,
  );
  const npmCli = await packageToolFileIdentity(
    path.join(npmPackageRoot, ...contract.launchers.npm.split("/")),
    "npm-cli",
    maximum,
  );
  const versions = await Promise.all([
    runPackageToolText(node.path, ["--version"], {
      cwd: staged.sourceRoot,
      env: npmEnvironment,
      label: "node-version",
    }),
    runPackageToolText(node.path, [npmCli.path, "--version"], {
      cwd: staged.sourceRoot,
      env: npmEnvironment,
      label: "npm-version",
    }),
    runPackageToolText(environmentContext.cargo, ["-Vv"], {
      cwd: staged.sourceRoot,
      env: cargoEnvironment,
      label: "cargo-version",
    }),
    runPackageToolText(environmentContext.rustc, ["-Vv"], {
      cwd: staged.sourceRoot,
      env: cargoEnvironment,
      label: "rustc-version",
    }),
    runPackageToolText(git.path, ["--version"], {
      cwd: sourceRoot,
      env: createPackageGitEnvironment(environmentContext),
      label: "git-version",
    }),
  ]);
  const cargoVersion = parsePackageRustVersion(versions[2], "cargo");
  const rustcVersion = parsePackageRustVersion(versions[3], "rustc");
  const gitVersion = versions[4].match(/^git version ([0-9A-Za-z.-]+)$/u)?.[1];
  if (
    versions[0] !== contract.node.version ||
    versions[1] !== contract.node.npmVersion ||
    cargoVersion.release !== contract.rust.release ||
    rustcVersion.release !== contract.rust.release ||
    cargoVersion.host !== contract.rust.host ||
    rustcVersion.host !== contract.rust.host ||
    cargoVersion.commit !== contract.rust.cargoCommit ||
    rustcVersion.commit !== contract.rust.rustcCommit ||
    !gitVersion
  ) {
    throw new Error("authenticated-package-tool-version-mismatch");
  }
  await runBounded(node.path, [npmCli.path, ...contract.npmCiArguments], {
    cwd: staged.sourceRoot,
    env: npmEnvironment,
    timeoutMs: toolTimeoutMs,
    label: "package-npm-ci",
    stdoutMaximum: frozen.package.observerBounds.toolStdoutBytes,
    stderrMaximum: frozen.package.observerBounds.toolStderrBytes,
  });
  const buildTools = await resolvePackageBuildTools(
    {
      sourceRoot: staged.sourceRoot,
      rustToolchainRoot,
      node: node.path,
      git: git.path,
    },
    frozen,
  );
  buildTools.powershell = powershell;
  const launcher = async (relativePath, label) =>
    packageToolFileIdentity(
      path.join(staged.sourceRoot, ...relativePath.split("/")),
      label,
      maximum,
    );
  const [
    typescript,
    vite,
    rustTree,
    cargoRegistryTree,
    npmTree,
    nodeModulesTree,
    msvcInputsTree,
    windowsSdkInputsTree,
    wixTree,
    nsisTree,
  ] = await Promise.all([
    launcher(contract.launchers.typescript, "typescript"),
    launcher(contract.launchers.vite, "vite"),
    observeAuthenticatedPackageTree({
      root: rustToolchainRoot,
      bounds: packageBuildTreeBounds(frozen, "rustToolchainBytes"),
    }),
    observeAuthenticatedPackageTree({
      root: path.join(cargoHome, "registry"),
      bounds: packageBuildTreeBounds(frozen, "cargoRegistryBytes"),
    }),
    observeAuthenticatedPackageTree({
      root: npmPackageRoot,
      bounds: packageBuildTreeBounds(frozen, "npmPackageBytes"),
    }),
    observeAuthenticatedPackageTree({
      root: path.join(staged.sourceRoot, "node_modules"),
      bounds: packageBuildTreeBounds(frozen, "nodeModulesBytes"),
    }),
    observeAuthenticatedPackageTreeSet({
      members: resolveAuthenticatedPackageTreeSetMembers({
        root: msvcToolsRoot,
        members: nativeContract.msvc.inputRoots,
      }),
      bounds: packageBuildTreeBounds(frozen, "msvcInputBytes"),
    }),
    observeAuthenticatedPackageTreeSet({
      members: resolveAuthenticatedPackageTreeSetMembers({
        root: windowsSdkRoot,
        members: nativeContract.windowsSdk.inputRoots,
        variables: { version: options.windowsSdkVersion },
      }),
      bounds: packageBuildTreeBounds(frozen, "windowsSdkInputBytes"),
    }),
    observeAuthenticatedPackageTree({
      root: wixToolsRoot,
      bounds: packageBuildTreeBounds(frozen, "wixToolsBytes"),
    }),
    observeAuthenticatedPackageTree({
      root: nsisToolsRoot,
      bounds: packageBuildTreeBounds(frozen, "nsisToolsBytes"),
    }),
  ]);
  const msvc = {
    cl: await packageToolFileIdentity(
      path.join(msvcToolsRoot, "bin", "Hostx64", "x64", "cl.exe"),
      "msvc-cl",
      maximum,
    ),
    link: await packageToolFileIdentity(
      path.join(msvcToolsRoot, "bin", "Hostx64", "x64", "link.exe"),
      "msvc-link",
      maximum,
    ),
  };
  const sdkBin = path.join(
    windowsSdkRoot,
    "bin",
    options.windowsSdkVersion,
    "x64",
  );
  const sdk = {
    rc: await packageToolFileIdentity(
      path.join(sdkBin, "rc.exe"),
      "sdk-rc",
      maximum,
    ),
    mt: await packageToolFileIdentity(
      path.join(sdkBin, "mt.exe"),
      "sdk-mt",
      maximum,
    ),
  };
  const { buildIdentity, buildIdentitySha256 } =
    deriveAuthenticatedPackageBuildIdentity({
      contract,
      versions: {
        node: versions[0],
        npm: versions[1],
        cargo: cargoVersion,
        rustc: rustcVersion,
        git: gitVersion,
      },
      buildTools,
      node,
      powershell,
      npmCli,
      git,
      launchers: { typescript, vite },
      msvc,
      sdk,
      trees: {
        rust: rustTree,
        cargoRegistry: cargoRegistryTree,
        npm: npmTree,
        nodeModules: nodeModulesTree,
        gitRuntime: gitRuntimeTree,
        msvcInputs: msvcInputsTree,
        windowsSdkInputs: windowsSdkInputsTree,
        wix: wixTree,
        nsis: nsisTree,
      },
      windowsSdkVersion: options.windowsSdkVersion,
    });
  Object.assign(buildTools, {
    createCargoEnvironment: (target) =>
      createPackageCargoEnvironment(environmentContext, target),
    gitEnvironment: createPackageGitEnvironment(environmentContext),
    toolEnvironment: createPackageGitEnvironment(environmentContext),
    metadataTargetDirectory: directories.metadataTarget,
  });
  installAuthenticatedPackageBundlers({
    buildTools,
    contract,
    frozen,
    wixTree,
    nsisTree,
  });
  await runBounded(
    node.path,
    [typescript.path, ...contract.frontend.typescriptArguments],
    {
      cwd: staged.sourceRoot,
      env: npmEnvironment,
      timeoutMs: toolTimeoutMs,
      label: "package-typescript",
      stdoutMaximum: frozen.package.observerBounds.toolStdoutBytes,
      stderrMaximum: frozen.package.observerBounds.toolStderrBytes,
    },
  );
  await runBounded(node.path, [vite.path, ...contract.frontend.viteArguments], {
    cwd: staged.sourceRoot,
    env: npmEnvironment,
    timeoutMs: toolTimeoutMs,
    label: "package-vite",
    stdoutMaximum: frozen.package.observerBounds.toolStdoutBytes,
    stderrMaximum: frozen.package.observerBounds.toolStderrBytes,
  });
  const watchedTrees = [
    [rustTree, "rustToolchainBytes"],
    [cargoRegistryTree, "cargoRegistryBytes"],
    [npmTree, "npmPackageBytes"],
    [nodeModulesTree, "nodeModulesBytes"],
    [gitRuntimeTree, "gitRuntimeBytes"],
    [wixTree, "wixToolsBytes"],
    [nsisTree, "nsisToolsBytes"],
  ];
  const watchedTreeSets = [
    [msvcInputsTree, "msvcInputBytes"],
    [windowsSdkInputsTree, "windowsSdkInputBytes"],
  ];
  const watchedFiles = [
    node,
    powershell,
    git,
    msvc.cl,
    msvc.link,
    sdk.rc,
    sdk.mt,
  ];
  return {
    sourceRoot: staged.sourceRoot,
    buildTools,
    buildIdentity,
    buildIdentitySha256,
    verifyInputs: async () => {
      const sourceAfter = buildSourceManifest(staged.sourceRoot);
      if (
        sourceAfter.aggregateSha256 !== sourceManifest.aggregateSha256 ||
        sourceAfter.fileCount !== sourceManifest.fileCount ||
        sourceAfter.totalBytes !== sourceManifest.totalBytes
      ) {
        throw new Error("staged-source-changed-during-package-campaign");
      }
      await verifyAuthenticatedPackageInputs({
        watchedTrees,
        watchedTreeSets,
        watchedFiles,
        frozen,
        maximumFileBytes: maximum,
      });
    },
  };
}
