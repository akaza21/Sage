import { createHash } from "node:crypto";
import { lstat, readFile, readdir } from "node:fs/promises";
import path from "node:path";
import process from "node:process";

import { cargoPackageFeatureErrors } from "./ade0006a-package-policy.mjs";

const root = process.cwd();
const failures = [];

async function readJson(relativePath) {
  return JSON.parse(await readFile(path.join(root, relativePath), "utf8"));
}

function fail(message) {
  failures.push(message);
}

async function filesBelow(relativeDirectory) {
  const directory = path.join(root, relativeDirectory);
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];

  for (const entry of entries) {
    const relativePath = path.join(relativeDirectory, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await filesBelow(relativePath)));
    } else {
      files.push(relativePath);
    }
  }

  return files;
}

function canonicalRelative(value) {
  return value.split(path.sep).join("/");
}

const betaAcceptanceRelativePath =
  "docs/work-items/BETA-0001-primary-beta-path/contracts/beta-acceptance-v1.json";
const betaAcceptanceBytes = await readFile(
  path.join(root, betaAcceptanceRelativePath),
);
const betaAcceptance = JSON.parse(betaAcceptanceBytes.toString("utf8"));
const betaAcceptanceSha256 = createHash("sha256")
  .update(betaAcceptanceBytes)
  .digest("hex");
if (
  betaAcceptanceSha256 !==
    "c947f075aea6ae44f425a9a1db983024bb4acebc7b1de64194e5d774776a6b7a" ||
  betaAcceptance.schemaVersion !== 1 ||
  betaAcceptance.status !== "frozen-thresholds-awaiting-candidate-verifier" ||
  betaAcceptance.workItem !== "BETA-0001"
) {
  fail("BETA-0001 acceptance contract drifted from its reviewed v1 bytes.");
}
const betaContractDirectory = path.dirname(
  path.join(root, betaAcceptanceRelativePath),
);
for (const authority of betaAcceptance.sharedAuthorities ?? []) {
  const authorityPath = path.resolve(
    betaContractDirectory,
    authority.relativePath,
  );
  try {
    const metadata = await lstat(authorityPath);
    if (!metadata.isFile() || metadata.isSymbolicLink()) {
      fail(`BETA-0001 ${authority.id} authority is not a regular file.`);
      continue;
    }
    const bytes = await readFile(authorityPath);
    const sha256 = createHash("sha256").update(bytes).digest("hex");
    if (authority.bytes !== bytes.length || authority.sha256 !== sha256) {
      fail(`BETA-0001 ${authority.id} authority bytes drifted.`);
    }
  } catch {
    fail(`BETA-0001 ${authority.id} authority is missing.`);
  }
}

const ownershipManifest = await readJson(
  "docs/work-items/ADE-0006A-acp-session-foundation/contracts/ownership-manifest-v1.json",
);
const ownershipScope = new Set(ownershipManifest.ownedFileScopes ?? []);
for (const file of ownershipManifest.ownedFileScopes ?? []) {
  try {
    const metadata = await lstat(path.join(root, file));
    if (!metadata.isFile() || metadata.isSymbolicLink()) {
      fail(`ADE-0006A owned file is not a regular file: ${file}`);
    }
  } catch {
    fail(`ADE-0006A owned file is missing: ${file}`);
  }
}
for (const directory of ownershipManifest.ownedDirectoryScopes ?? []) {
  for (const file of await filesBelow(directory)) {
    ownershipScope.add(canonicalRelative(file));
  }
}
const adeScriptName =
  /^(?:ade0006a|collect-ade0006a|validate-ade0006a|test-ade0006a)[a-z0-9.-]*\.(?:mjs|ps1)$/;
for (const entry of await readdir(path.join(root, "scripts"), {
  withFileTypes: true,
})) {
  const relativePath = `scripts/${entry.name}`;
  if (
    entry.isFile() &&
    adeScriptName.test(entry.name) &&
    !ownershipScope.has(relativePath)
  ) {
    fail(`ADE-0006A script is outside ownership: ${relativePath}`);
  }
}
const ownershipRules = ownershipManifest.rules ?? [];
const ownerNames = new Set();
for (const rule of ownershipRules) {
  if (
    typeof rule.owner !== "string" ||
    rule.owner.length === 0 ||
    ownerNames.has(rule.owner)
  ) {
    fail("ADE-0006A ownership rule names must be unique and nonempty.");
  }
  ownerNames.add(rule.owner);
  for (const prefix of rule.prefixes ?? []) {
    if (
      !prefix.endsWith("/") ||
      prefix.includes("\\") ||
      prefix.includes("..")
    ) {
      fail(`ADE-0006A ownership prefix is noncanonical: ${prefix}`);
    }
  }
  for (const file of rule.files ?? []) {
    if (
      file.includes("\\") ||
      file.includes("..") ||
      !ownershipScope.has(file)
    ) {
      fail(`ADE-0006A ownership file is outside its closed scope: ${file}`);
    }
  }
}
for (const file of ownershipScope) {
  const owners = ownershipRules.filter(
    (rule) =>
      (rule.files ?? []).includes(file) ||
      (rule.prefixes ?? []).some((prefix) => file.startsWith(prefix)),
  );
  if (owners.length !== 1) {
    fail(
      `ADE-0006A ownership must resolve exactly once for ${file}; found ${owners.length}.`,
    );
  }
}

const contractDirectory =
  "docs/work-items/ADE-0006A-acp-session-foundation/contracts";
const contractManifest = await readJson(
  `${contractDirectory}/contract-manifest-v1.json`,
);
const declaredContractFiles = contractManifest.files?.map(
  (entry) => entry.relativePath,
);
const actualContractFiles = (await readdir(path.join(root, contractDirectory)))
  .filter((file) => file !== "contract-manifest-v1.json")
  .sort();
if (
  contractManifest.selfIncluded !== false ||
  contractManifest.fileCount !== declaredContractFiles?.length ||
  JSON.stringify(declaredContractFiles) !== JSON.stringify(actualContractFiles)
) {
  fail("ADE-0006A contract manifest membership is stale or unordered.");
} else {
  let logicalBytes = 0;
  for (const entry of contractManifest.files) {
    const bytes = await readFile(
      path.join(root, contractDirectory, entry.relativePath),
    );
    logicalBytes += bytes.length;
    const digest = createHash("sha256").update(bytes).digest("hex");
    if (entry.bytes !== bytes.length || entry.sha256 !== digest) {
      fail(`ADE-0006A contract manifest mismatch: ${entry.relativePath}`);
    }
  }
  if (contractManifest.logicalBytes !== logicalBytes) {
    fail("ADE-0006A contract manifest logical byte total is stale.");
  }
}

const tauriConfig = await readJson("src-tauri/tauri.conf.json");
const tauriWindowsConfig = await readJson("src-tauri/tauri.windows.conf.json");
const controlTauriConfig = await readJson(
  "tools/footprint-control/tauri.conf.json",
);
const controlWindowsConfig = await readJson(
  "tools/footprint-control/tauri.windows.package.conf.json",
);
const fixtureVectors = await readJson(
  "docs/work-items/ADE-0006A-acp-session-foundation/contracts/fixture-vectors-v1.json",
);
const packageContract = await readJson(
  "docs/work-items/ADE-0006A-acp-session-foundation/contracts/package-contract-v1.json",
);
const evidencePolicy = await readJson(
  "docs/work-items/ADE-0006A-acp-session-foundation/contracts/evidence-policy-v1.json",
);
const packageCargoToml = await readFile(
  path.join(root, "src-tauri/Cargo.toml"),
  "utf8",
);
const controlCargoToml = await readFile(
  path.join(root, "tools/footprint-control/Cargo.toml"),
  "utf8",
);
const csp = tauriConfig.app?.security?.csp;
const devCsp = tauriConfig.app?.security?.devCsp;

if (csp === null || csp === undefined) {
  fail("Tauri CSP must be enabled.");
}

if (typeof csp === "string" && csp.trim().length === 0) {
  fail("Tauri CSP must not be empty.");
}

const productionCspText = JSON.stringify(csp);
for (const forbidden of [
  "ws://127.0.0.1:1420",
  "http://127.0.0.1:1420",
  "ws://localhost",
  "http://localhost",
  "'unsafe-eval'",
]) {
  if (productionCspText.includes(forbidden)) {
    fail(`Production CSP contains forbidden development source: ${forbidden}`);
  }
}

if (
  csp?.["script-src"] !== "'self'" ||
  csp?.["connect-src"] !==
    "'self' ipc: http://ipc.localhost http://tauri.localhost"
) {
  fail(
    "Production script/connect CSP must match the reviewed exact allowlist.",
  );
}

if (
  devCsp?.["connect-src"] !==
  "'self' ipc: http://ipc.localhost http://tauri.localhost ws://127.0.0.1:1420 http://127.0.0.1:1420"
) {
  fail("Development CSP must add only the exact loopback Vite origins.");
}

const expectedWindowsTargets = ["msi", "nsis"];
const windowsBundle = tauriWindowsConfig.bundle;
const windowsWebview = windowsBundle?.windows?.webviewInstallMode;
if (
  JSON.stringify(windowsBundle?.targets) !==
    JSON.stringify(expectedWindowsTargets) ||
  windowsWebview?.type !== "downloadBootstrapper" ||
  windowsWebview?.silent !== true
) {
  fail(
    "Windows production-default packaging must explicitly use MSI, NSIS, and the silent WebView2 download bootstrapper.",
  );
}

const declaredPackageMembers = [
  ...(tauriConfig.bundle?.resources ?? []),
  ...(tauriConfig.bundle?.externalBin ?? []),
  ...(windowsBundle?.resources ?? []),
  ...(windowsBundle?.externalBin ?? []),
];
if (declaredPackageMembers.length !== 0) {
  fail("Production package resources and external binaries must remain empty.");
}

const cargoBinBlocks =
  packageCargoToml.match(
    /\[\[bin\]\][\s\S]*?(?=\r?\n\[(?:\[bin\]|features|profile\.)|$)/g,
  ) ?? [];
const helperBinBlocks = cargoBinBlocks.filter((block) =>
  /^name = "sage-codex-0145-elevated-helper"$/m.test(block),
);
const cargoFeaturesBlock =
  packageCargoToml.match(/\[features\][\s\S]*?(?=\r?\n\[|$)/)?.[0] ?? "";
if (
  helperBinBlocks.length !== 1 ||
  !/^path = "src\/bin\/codex_provisioning_helper\.rs"$/m.test(
    helperBinBlocks[0],
  ) ||
  !/^required-features = \["codex-provisioning-helper"\]$/m.test(
    helperBinBlocks[0],
  ) ||
  !/^default = \[\]$/m.test(cargoFeaturesBlock) ||
  !/^codex-provisioning-helper = \[\]$/m.test(cargoFeaturesBlock)
) {
  fail(
    "The Codex elevated helper must remain one explicit feature-only binary outside the default production package.",
  );
}

const helperBoundaryFiles = [
  "src-tauri/src/codex_helper_boundary.rs",
  ...(await filesBelow("src-tauri/src/codex_helper_boundary")).map(
    canonicalRelative,
  ),
];
const helperBoundarySource = (
  await Promise.all(
    helperBoundaryFiles.map((file) => readFile(path.join(root, file), "utf8")),
  )
).join("\n");
for (const forbiddenReference of [
  "ElevatedHelperProcess",
  "acp::bindings::codex::provisioning",
  "provisioning::helper",
]) {
  if (helperBoundarySource.includes(forbiddenReference)) {
    fail(
      `The inert Codex helper boundary must not reach the mutation core: ${forbiddenReference}`,
    );
  }
}

const candidatePackage = packageContract.participants?.candidate;
const controlPackage = packageContract.participants?.control;
const packageArtifactKinds = candidatePackage?.artifacts?.map(
  ({ kind }) => kind,
);
const packageArtifactPaths = candidatePackage?.artifacts?.map(
  ({ relativePath }) => relativePath,
);
const controlArtifactPaths = controlPackage?.artifacts?.map(
  ({ relativePath }) => relativePath,
);
const controlBundle = controlWindowsConfig.bundle;
const controlWebview = controlBundle?.windows?.webviewInstallMode;
if (
  packageContract.schemaVersion !== 1 ||
  packageContract.platform !== "windows" ||
  packageContract.productionBuild?.profile !== "release" ||
  packageContract.productionBuild?.defaultFeaturesEnabled !== true ||
  JSON.stringify(packageContract.productionBuild?.enabledFeatures) !==
    JSON.stringify(["custom-protocol"]) ||
  JSON.stringify(packageContract.productionBuild?.bundleTargets) !==
    JSON.stringify(expectedWindowsTargets) ||
  candidatePackage?.applicationRoot !== "." ||
  candidatePackage?.cargoManifest !== "src-tauri/Cargo.toml" ||
  candidatePackage?.configuration?.base !== "src-tauri/tauri.conf.json" ||
  candidatePackage?.configuration?.platformOverlay !==
    "src-tauri/tauri.windows.conf.json" ||
  controlPackage?.applicationRoot !== "tools/footprint-control" ||
  controlPackage?.cargoManifest !== "tools/footprint-control/Cargo.toml" ||
  controlPackage?.configuration?.base !==
    "tools/footprint-control/tauri.conf.json" ||
  controlPackage?.configuration?.platformOverlay !==
    "tools/footprint-control/tauri.windows.package.conf.json" ||
  JSON.stringify(packageArtifactKinds) !==
    JSON.stringify(["executable", "msi", "nsis"]) ||
  JSON.stringify(packageArtifactPaths) !==
    JSON.stringify([
      "release/sage.exe",
      "release/bundle/msi/Sage_0.1.0_x64_en-US.msi",
      "release/bundle/nsis/Sage_0.1.0_x64-setup.exe",
    ]) ||
  JSON.stringify(controlArtifactPaths) !==
    JSON.stringify([
      "release/sage-footprint-control.exe",
      "release/bundle/msi/Sage Footprint Control_0.1.0_x64_en-US.msi",
      "release/bundle/nsis/Sage Footprint Control_0.1.0_x64-setup.exe",
    ]) ||
  packageContract.artifactSet?.requiredCount !== 3 ||
  JSON.stringify(controlBundle?.targets) !==
    JSON.stringify(expectedWindowsTargets) ||
  controlBundle?.active !== true ||
  controlWebview?.type !== "downloadBootstrapper" ||
  controlWebview?.silent !== true ||
  JSON.stringify(controlTauriConfig.app?.security?.csp) !==
    JSON.stringify(tauriConfig.app?.security?.csp) ||
  packageContract.webviewProvisioning?.mode !== "downloadBootstrapper" ||
  packageContract.webviewProvisioning?.silent !== true ||
  packageContract.webviewProvisioning?.whenRuntimeAbsent !== true ||
  packageContract.webviewProvisioning?.networkRequiredWhenTriggered !== true ||
  packageContract.webviewProvisioning?.bundledPayloadCount !== 0 ||
  packageContract.webviewProvisioning?.bundledPayloadBytes !== 0 ||
  packageContract.fixtureExclusion?.productionFeatureSetContainsFixture !==
    false ||
  packageContract.fixtureExclusion?.canonicalFeature !== "acp-fixture" ||
  JSON.stringify(packageContract.fixtureExclusion?.canonicalBasenames) !==
    JSON.stringify([
      "acp-adversarial-fixture.exe",
      "acp-adversarial-fixture-x86_64-pc-windows-msvc.exe",
    ]) ||
  packageContract.fixtureExclusion?.metadataDeclarationCount !== 0 ||
  packageContract.fixtureExclusion?.extractedMemberBasenameCountPerInstaller !==
    0 ||
  packageContract.fixtureExclusion
    ?.extractedPayloadHashMatchesFixturePerInstaller !== 0 ||
  packageContract.installedExecutableIdentity?.algorithm !==
    "tauri-bundle-type-patch-v1" ||
  packageContract.installedExecutableIdentity?.sourceMarker !==
    "__TAURI_BUNDLE_TYPE_VAR_UNK" ||
  packageContract.installedExecutableIdentity?.sourceMarkerOccurrenceCount !==
    1 ||
  packageContract.installedExecutableIdentity?.targetMarkers?.msi !==
    "__TAURI_BUNDLE_TYPE_VAR_MSI" ||
  packageContract.installedExecutableIdentity?.targetMarkers?.nsis !==
    "__TAURI_BUNDLE_TYPE_VAR_NSS" ||
  packageContract.inspection?.installedPayloadProjection?.msi?.method !==
    "wix-dark-install-root-v1" ||
  packageContract.inspection?.installedPayloadProjection?.nsis?.method !==
    "nsis-installed-tree-v1" ||
  packageContract.retention?.rawReservation?.sourceManifestFiles !== 1 ||
  packageContract.retention?.rawReservation?.sourceManifestBytes !==
    8 * 1024 * 1024 ||
  packageContract.retention?.rawReservation?.sourceManifestEntries !== 4096 ||
  packageContract.retention?.rawReservation?.installerMemberSets !== 4 ||
  packageContract.retention?.rawReservation?.memberSidecarFiles !== 32 ||
  packageContract.retention?.rawReservation?.memberSidecarBytes !==
    24 * 1024 * 1024 ||
  packageContract.retention?.rawReservation?.totalFiles !== 33 ||
  packageContract.retention?.rawReservation?.totalBytes !== 32 * 1024 * 1024 ||
  evidencePolicy.packageCampaignRetention?.ledgerBytes !== 64 * 1024 ||
  evidencePolicy.packageCampaignRetention?.ledgerEntries !== 16 ||
  evidencePolicy.packageCampaignRetention?.reservation?.rawFiles !== 33 ||
  evidencePolicy.packageCampaignRetention?.reservation?.rawBytes !==
    32 * 1024 * 1024 ||
  evidencePolicy.packageCampaignRetention?.reservation
    ?.sourceManifestEntries !== 4096 ||
  evidencePolicy.packageCampaignRetention?.reservation?.curatedFiles !== 1 ||
  evidencePolicy.packageCampaignRetention?.reservation?.curatedBytes !==
    evidencePolicy.campaignBounds?.curatedBytes
) {
  fail("ADE-0006A package contract is internally inconsistent.");
}
for (const error of cargoPackageFeatureErrors(
  packageCargoToml,
  packageContract,
  "candidate",
)) {
  fail(error);
}
for (const error of cargoPackageFeatureErrors(
  controlCargoToml,
  packageContract,
  "control",
)) {
  fail(error);
}

const packageFixture = fixtureVectors.cases?.find(
  ({ id }) => id === "default-package-fixture-exclusion",
);
const expectedPackageOracle = {
  productionArtifactKinds: packageArtifactKinds,
  fixtureExecutableCount: 0,
  fixtureMetadataCount:
    packageContract.fixtureExclusion.metadataDeclarationCount,
  productionFeatureSetContainsAcpFixture:
    packageContract.fixtureExclusion.productionFeatureSetContainsFixture,
  extractedFixtureHashMatches:
    packageContract.fixtureExclusion
      .extractedPayloadHashMatchesFixturePerInstaller,
  msiExecutableMatchesExpectedBundleProjection: true,
  nsisExecutableMatchesExpectedBundleProjection: true,
  webviewProvisioningMode: packageContract.webviewProvisioning.mode,
  webviewProvisioningWhenRuntimeAbsent:
    packageContract.webviewProvisioning.whenRuntimeAbsent,
  webviewProvisioningNetworkRequired:
    packageContract.webviewProvisioning.networkRequiredWhenTriggered,
  bundledWebviewPayloadCount:
    packageContract.webviewProvisioning.bundledPayloadCount,
  bundledWebviewPayloadBytes:
    packageContract.webviewProvisioning.bundledPayloadBytes,
};
if (
  JSON.stringify(packageFixture?.oracle) !==
  JSON.stringify(expectedPackageOracle)
) {
  fail("Package fixture oracle does not match the frozen package contract.");
}

const capabilities = await readJson("src-tauri/capabilities/main.json");
const allowedPermissions = new Set(["core:default"]);

for (const permission of capabilities.permissions ?? []) {
  const identifier =
    typeof permission === "string" ? permission : permission.identifier;
  if (!allowedPermissions.has(identifier)) {
    fail(`Unexpected Tauri permission: ${identifier}`);
  }
}

const packageJson = await readJson("package.json");
const allPackages = {
  ...(packageJson.dependencies ?? {}),
  ...(packageJson.devDependencies ?? {}),
};
const forbiddenPackages = [
  "electron",
  "monaco-editor",
  "node-pty",
  "simple-git",
  "isomorphic-git",
];

for (const dependency of forbiddenPackages) {
  if (dependency in allPackages) {
    fail(`Forbidden core dependency: ${dependency}`);
  }
}

const cargoToml = await readFile(
  path.join(root, "src-tauri/Cargo.toml"),
  "utf8",
);
const nativeLibrary = await readFile(
  path.join(root, "src-tauri/src/lib.rs"),
  "utf8",
);
const viteConfig = await readFile(path.join(root, "vite.config.ts"), "utf8");
const acpProtocolContract = await readJson(
  "docs/work-items/ADE-0006A-acp-session-foundation/contracts/protocol-contract-v1.json",
);
const projectionEvent = acpProtocolContract.projectionEvent?.name;
const nativeState = await readFile(
  path.join(root, "src-tauri/src/state.rs"),
  "utf8",
);
const frontendAcpDesktop = await readFile(
  path.join(root, "src/services/acpDesktop.ts"),
  "utf8",
);

const cspFixture = fixtureVectors.cases?.find(
  (fixtureCase) => fixtureCase.id === "csp-production-and-development",
);
const cspBuilds = cspFixture?.steps?.[0]?.builds;
const productionConnectSrc = csp?.["connect-src"]?.split(/\s+/u) ?? [];
const developmentConnectSrc = devCsp?.["connect-src"]?.split(/\s+/u) ?? [];
const productionConnectSet = new Set(productionConnectSrc);
const cspObservation = {
  cspDisabled: csp === null || csp === undefined,
  unsafeEvalAdded:
    String(csp?.["script-src"]).includes("unsafe-eval") ||
    String(devCsp?.["script-src"]).includes("unsafe-eval"),
  productionConnectSrcExact: productionConnectSrc,
  developmentConnectSrcAdditionalExact: developmentConnectSrc.filter(
    (source) => !productionConnectSet.has(source),
  ),
  productionForbidden: [
    "ws://127.0.0.1:1420",
    "http://127.0.0.1:1420",
    "http://localhost:*",
    "ws://localhost:*",
    "remote-script",
    "new-scheme",
    "unsafe-eval",
  ].filter((source) => !productionCspText.includes(source)),
  scriptSrcExact: csp?.["script-src"]?.split(/\s+/u) ?? [],
  rawAdapterEndpointExposed:
    /commands::(?:raw|fixture|adapter)[A-Za-z0-9_]*/u.test(nativeLibrary),
};
if (
  JSON.stringify(cspBuilds) !==
    JSON.stringify([
      "production-default",
      "development",
      "feature-gated-characterization",
    ]) ||
  JSON.stringify(cspObservation) !== JSON.stringify(cspFixture?.oracle)
) {
  fail("Frozen production/development CSP fixture oracle did not match.");
}

if (
  projectionEvent !== "acp-projection-changed-v1" ||
  !nativeState.includes(`emit(ACP_PROJECTION_CHANGED_EVENT, event)`) ||
  !frontendAcpDesktop.includes(
    `ACP_PROJECTION_CHANGED_EVENT = "${projectionEvent}"`,
  )
) {
  fail(
    "Native and frontend ACP projection events must match the frozen contract.",
  );
}

if (/\bsourcemap\s*:\s*true\b/u.test(viteConfig)) {
  fail(
    "Production source maps must be explicitly opt-in, not unconditionally packaged.",
  );
}

if (/^\s*git2\s*=/m.test(cargoToml)) {
  fail("git2 is forbidden; Sage uses the installed Git CLI.");
}

if (
  !/agent-client-protocol-schema\s*=\s*\{[^}]*version\s*=\s*"=1\.4\.0"[^}]*optional\s*=\s*true[^}]*default-features\s*=\s*false/s.test(
    cargoToml,
  )
) {
  fail(
    "The ACP boundary must use the exact feature-disabled schema v1.4.0 crate.",
  );
}

if (/^agent-client-protocol\s*=/mu.test(cargoToml)) {
  fail(
    "The ACP runtime crate is forbidden; Sage owns bounded transport and process control.",
  );
}

if (
  !/^acp-schema\s*=\s*\["dep:agent-client-protocol-schema"\]/mu.test(cargoToml)
) {
  fail(
    "The ACP schema crate must remain behind the isolated acp-schema feature.",
  );
}

if (/^acp-runtime\s*=/mu.test(cargoToml)) {
  fail("The rejected acp-runtime feature must not be reachable.");
}

const schemaDeclaration = cargoToml.match(
  /^agent-client-protocol-schema\s*=\s*\{([^}]*)\}/mu,
)?.[1];
if (
  schemaDeclaration === undefined ||
  /(?:^|[,\s])features\s*=/u.test(schemaDeclaration) ||
  /\b(?:unstable|tracing)\b/u.test(schemaDeclaration)
) {
  fail("ACP schema tracing and unstable features must remain disabled.");
}

const tracingDeclaration = cargoToml.match(
  /^tracing-subscriber\s*=\s*\{([^}]*)\}/mu,
)?.[1];
if (
  tracingDeclaration === undefined ||
  !/version\s*=\s*"=0\.3\.23"/u.test(tracingDeclaration) ||
  !/default-features\s*=\s*false/u.test(tracingDeclaration) ||
  !/features\s*=\s*\["fmt"\]/u.test(tracingDeclaration) ||
  /env-filter/u.test(tracingDeclaration)
) {
  fail(
    "Tracing must use exact fmt-only 0.3.23 without default or ambient filters.",
  );
}

const windowsDeclaration = cargoToml.match(
  /^windows-sys\s*=\s*\{([\s\S]*?)^\] \}\s*$/mu,
)?.[1];
const windowsFeatures = [
  "Win32_Foundation",
  "Win32_Security",
  "Win32_Storage_FileSystem",
  "Win32_System_JobObjects",
  "Win32_System_Pipes",
  "Win32_System_Performance",
  "Win32_System_SystemInformation",
  "Win32_System_Threading",
];
const declaredWindowsFeatures = windowsDeclaration
  ?.split(/features\s*=\s*\[/u)[1]
  ?.match(/"[^"]+"/gu)
  ?.map((feature) => feature.slice(1, -1));
if (
  windowsDeclaration === undefined ||
  !/version\s*=\s*"=0\.61\.2"/u.test(windowsDeclaration) ||
  !/default-features\s*=\s*false/u.test(windowsDeclaration) ||
  declaredWindowsFeatures === undefined ||
  declaredWindowsFeatures.length !== windowsFeatures.length ||
  declaredWindowsFeatures.some(
    (feature, index) => feature !== windowsFeatures[index],
  )
) {
  fail(
    "windows-sys must expose exactly the reviewed Windows containment API set.",
  );
}

const productionFiles = [
  ...(await filesBelow("src")),
  ...(await filesBelow("src-tauri/src")),
].filter(
  (file) =>
    /\.(?:ts|tsx|rs)$/.test(file) &&
    !/\.test\.tsx?$/u.test(file) &&
    !/(?:^|[\\/])tests(?:[\\/]|\.rs$)/u.test(file) &&
    !/(?:^|[\\/])[^\\/]+_tests\.rs$/u.test(file),
);

const productionLineExceptions = new Map([
  [
    "src-tauri/src/editor_persistence.rs",
    {
      limit: 500,
      reason: "existing durable editor transaction boundary",
    },
  ],
  [
    "src-tauri/src/acp/fixture/host/store_bounds.rs",
    {
      limit: 475,
      reason: "closed bounded adversarial fixture dispatch table",
    },
  ],
  [
    "src-tauri/src/acp/bindings/claude/translator.rs",
    {
      limit: 425,
      reason: "closed protocol translation table",
    },
  ],
]);

function productionLineCount(source) {
  // Preserve newlines while removing block comments, then ignore blank and
  // line-comment-only rows. Inline comments still count as production code.
  const withoutBlockComments = source.replace(/\/\*[\s\S]*?\*\//gu, (comment) =>
    comment.replace(/[^\r\n]/gu, ""),
  );
  return withoutBlockComments.split(/\r?\n/u).filter((line) => {
    const trimmed = line.trim();
    return trimmed.length > 0 && !trimmed.startsWith("//");
  }).length;
}

for (const file of productionFiles) {
  const source = await readFile(path.join(root, file), "utf8");
  const forbiddenRuntimePatterns = [
    ["agent_client_protocol::Builder", "ACP runtime Builder"],
    ["agent_client_protocol::Stdio", "ACP runtime Stdio"],
    ["agent_client_protocol::ConnectionTo", "ACP runtime ConnectionTo"],
    ["AsyncBufReadExt::lines", "unbounded async line reader"],
    ["mpsc::unbounded_channel", "unbounded MPSC channel"],
    ["EnvFilter", "ambient tracing filter"],
    ["RUST_LOG", "ambient tracing configuration"],
  ];
  for (const [pattern, description] of forbiddenRuntimePatterns) {
    if (source.includes(pattern)) {
      fail(`${file} contains forbidden ${description}.`);
    }
  }
  // Rust commonly keeps unit tests in a cfg-gated child module. Those lines
  // are not linked into production and should not mask an oversized runtime
  // module; the runtime portion remains subject to the same budget.
  const productionSource = file.endsWith(".rs")
    ? source.split(/\r?\n#\[cfg\(test\)\]\r?\nmod tests \{/u, 1)[0]
    : source;
  const lineCount = productionLineCount(productionSource);
  const normalizedFile = file.split(path.sep).join("/");
  const exception = productionLineExceptions.get(normalizedFile);
  const lineLimit = exception?.limit ?? 400;
  if (lineCount > lineLimit) {
    const exceptionContext = exception
      ? `; reviewed exception: ${exception.reason}`
      : "";
    fail(
      `${file} has ${lineCount} non-comment lines (limit ${lineLimit})${exceptionContext}.`,
    );
  }
}

if (failures.length > 0) {
  for (const failure of failures) {
    console.error(`policy: ${failure}`);
  }
  process.exitCode = 1;
} else {
  console.log(
    `policy: passed (${productionFiles.length} production modules checked)`,
  );
}
