import {
  exactKeys,
  recordDigest,
  safeInteger,
  SHA256,
} from "./ade0006a-c0b-common.mjs";

function validateTreeSummary(errors, value, label, frozen, bytesKey) {
  const bounds = frozen.package.authenticatedBuild.bounds;
  if (!SHA256.test(value?.treeSha256 ?? "")) {
    errors.push(`${label}.treeSha256 is invalid`);
  }
  safeInteger(
    errors,
    value?.fileCount,
    `${label}.fileCount`,
    1,
    bounds.treeMembers,
  );
  safeInteger(errors, value?.bytes, `${label}.bytes`, 1, bounds[bytesKey]);
}

function validHashes(value, keys) {
  return keys.every((key) => SHA256.test(value?.[key] ?? ""));
}

export function validateAuthenticatedPackageBuildIdentity(
  errors,
  value,
  frozen,
) {
  const contract = frozen.package.authenticatedBuild;
  const label = "receipt.buildIdentity";
  exactKeys(
    errors,
    value,
    [
      "schemaVersion",
      "recipe",
      "sourceStage",
      "environmentPolicySha256",
      "rustToolchain",
      "cargoRegistry",
      "node",
      "powershell",
      "npm",
      "nodeModules",
      "launchers",
      "git",
      "msvc",
      "windowsSdk",
      "bundlers",
    ],
    label,
  );
  if (
    value?.schemaVersion !== 1 ||
    value?.recipe !== contract.recipe ||
    value?.sourceStage !== contract.sourceStage ||
    value?.environmentPolicySha256 !== recordDigest(contract.environment)
  ) {
    errors.push(`${label} header or environment policy is invalid`);
  }
  exactKeys(
    errors,
    value?.rustToolchain,
    [
      "directoryName",
      "release",
      "host",
      "cargoVersion",
      "cargoCommit",
      "cargoExecutableSha256",
      "rustcVersion",
      "rustcCommit",
      "rustcExecutableSha256",
      "treeSha256",
      "fileCount",
      "bytes",
    ],
    `${label}.rustToolchain`,
  );
  validateTreeSummary(
    errors,
    value?.rustToolchain,
    `${label}.rustToolchain`,
    frozen,
    "rustToolchainBytes",
  );
  if (
    value?.rustToolchain?.directoryName !==
      contract.rust.toolchainDirectoryName ||
    value?.rustToolchain?.release !== contract.rust.release ||
    value?.rustToolchain?.host !== contract.rust.host ||
    value?.rustToolchain?.cargoVersion !== contract.rust.release ||
    value?.rustToolchain?.rustcVersion !== contract.rust.release ||
    value?.rustToolchain?.cargoCommit !== contract.rust.cargoCommit ||
    value?.rustToolchain?.rustcCommit !== contract.rust.rustcCommit ||
    !validHashes(value?.rustToolchain, [
      "cargoExecutableSha256",
      "rustcExecutableSha256",
    ]) ||
    value?.rustToolchain?.cargoExecutableSha256 ===
      value?.rustToolchain?.rustcExecutableSha256
  ) {
    errors.push(`${label}.rustToolchain is not the pinned real toolchain`);
  }
  exactKeys(
    errors,
    value?.cargoRegistry,
    ["treeSha256", "fileCount", "bytes"],
    `${label}.cargoRegistry`,
  );
  validateTreeSummary(
    errors,
    value?.cargoRegistry,
    `${label}.cargoRegistry`,
    frozen,
    "cargoRegistryBytes",
  );
  exactKeys(
    errors,
    value?.node,
    ["version", "executableSha256"],
    `${label}.node`,
  );
  exactKeys(
    errors,
    value?.powershell,
    ["executableSha256"],
    `${label}.powershell`,
  );
  exactKeys(
    errors,
    value?.npm,
    ["version", "cliSha256", "treeSha256", "fileCount", "bytes"],
    `${label}.npm`,
  );
  exactKeys(
    errors,
    value?.nodeModules,
    ["treeSha256", "fileCount", "bytes"],
    `${label}.nodeModules`,
  );
  validateTreeSummary(
    errors,
    value?.npm,
    `${label}.npm`,
    frozen,
    "npmPackageBytes",
  );
  validateTreeSummary(
    errors,
    value?.nodeModules,
    `${label}.nodeModules`,
    frozen,
    "nodeModulesBytes",
  );
  if (
    value?.node?.version !== contract.node.version ||
    value?.npm?.version !== contract.node.npmVersion ||
    !SHA256.test(value?.node?.executableSha256 ?? "") ||
    !SHA256.test(value?.npm?.cliSha256 ?? "") ||
    !SHA256.test(value?.powershell?.executableSha256 ?? "")
  ) {
    errors.push(`${label} Node, npm, or PowerShell identity is invalid`);
  }
  exactKeys(
    errors,
    value?.launchers,
    ["typescriptSha256", "viteSha256", "tauriJsSha256", "tauriNativeSha256"],
    `${label}.launchers`,
  );
  exactKeys(
    errors,
    value?.git,
    ["version", "executableSha256", "treeSha256", "fileCount", "bytes"],
    `${label}.git`,
  );
  exactKeys(
    errors,
    value?.msvc,
    ["version", "clSha256", "linkSha256", "treeSha256", "fileCount", "bytes"],
    `${label}.msvc`,
  );
  exactKeys(
    errors,
    value?.windowsSdk,
    ["version", "rcSha256", "mtSha256", "treeSha256", "fileCount", "bytes"],
    `${label}.windowsSdk`,
  );
  validateTreeSummary(
    errors,
    value?.git,
    `${label}.git`,
    frozen,
    "gitRuntimeBytes",
  );
  validateTreeSummary(
    errors,
    value?.msvc,
    `${label}.msvc`,
    frozen,
    "msvcInputBytes",
  );
  validateTreeSummary(
    errors,
    value?.windowsSdk,
    `${label}.windowsSdk`,
    frozen,
    "windowsSdkInputBytes",
  );
  if (
    !validHashes(value?.launchers, Object.keys(value?.launchers ?? {})) ||
    value?.git?.version !== contract.git.version ||
    !SHA256.test(value?.git?.executableSha256 ?? "") ||
    value?.msvc?.version !== contract.nativeTools.msvc.directoryName ||
    !validHashes(value?.msvc, ["clSha256", "linkSha256"]) ||
    value?.windowsSdk?.version !== contract.nativeTools.windowsSdk.version ||
    !validHashes(value?.windowsSdk, ["rcSha256", "mtSha256"])
  ) {
    errors.push(`${label} launch or native tool identity is invalid`);
  }
  exactKeys(errors, value?.bundlers, ["wix", "nsis"], `${label}.bundlers`);
  exactKeys(
    errors,
    value?.bundlers?.wix,
    [
      "treeSha256",
      "fileCount",
      "bytes",
      "candleSha256",
      "lightSha256",
      "darkSha256",
    ],
    `${label}.bundlers.wix`,
  );
  exactKeys(
    errors,
    value?.bundlers?.nsis,
    ["treeSha256", "fileCount", "bytes", "launcherSha256", "compilerSha256"],
    `${label}.bundlers.nsis`,
  );
  validateTreeSummary(
    errors,
    value?.bundlers?.wix,
    `${label}.bundlers.wix`,
    frozen,
    "wixToolsBytes",
  );
  validateTreeSummary(
    errors,
    value?.bundlers?.nsis,
    `${label}.bundlers.nsis`,
    frozen,
    "nsisToolsBytes",
  );
  if (
    !validHashes(value?.bundlers?.wix, [
      "candleSha256",
      "lightSha256",
      "darkSha256",
    ]) ||
    !validHashes(value?.bundlers?.nsis, ["launcherSha256", "compilerSha256"])
  ) {
    errors.push(`${label} bundler identity is invalid`);
  }
}
