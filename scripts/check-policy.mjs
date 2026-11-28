import { execFileSync } from "node:child_process";
import { readFile, readdir } from "node:fs/promises";
import path from "node:path";
import process from "node:process";

import { acpTransportParityErrors } from "./acp-transport-parity.mjs";
import { browserTestPolicyErrors } from "./browser-test-policy.mjs";
import { cargoFeaturePolicyErrors } from "./cargo-feature-policy.mjs";
import { testSupportGraphErrors } from "./cargo-test-support-policy.mjs";
import { cssHygienePolicyErrors } from "./css-hygiene-policy.mjs";
import { cssInlineControlPolicyErrors } from "./css-inline-control-policy.mjs";
import { cssScalePolicyErrors } from "./css-scale-policy.mjs";
import { cssTokenPolicyErrors } from "./css-token-policy.mjs";
import {
  cssHighlightPolicyErrors,
  discoverSourceFilesWithCssBound,
} from "./css-highlight-policy.mjs";
import { workbenchTransportParityErrors } from "./workbench-transport-parity.mjs";
import { q02SharedHelperPolicyErrors } from "./q02-shared-helpers-policy.mjs";
import { packageResourcePolicyErrors } from "./package-resource-policy.mjs";

const reviewedSpawnModules = new Map([
  ["crates/sage-git/src/process.rs", "Git CLI execution boundary"],
  [
    "src-tauri/src/acp/process/unix/spawn.rs",
    "Contained ACP harness launch on Unix",
  ],
  ["src-tauri/src/user_terminal/unix.rs", "User terminal shell launch on Unix"],
  [
    "src-tauri/src/app_preview/lifecycle.rs",
    "User-requested app preview server",
  ],
  [
    "src-tauri/src/harness_discovery/version_probe.rs",
    "Bounded harness version discovery",
  ],
  [
    "crates/sage-workspace/src/search/helper_process.rs",
    "Bounded workspace search helper",
  ],
  [
    "src-tauri/src/codex_helper_boundary/caller.rs",
    "Provisioning helper caller",
  ],
  ["src-tauri/src/commands/acp/claude_auth.rs", "Claude Code sign-in"],
  [
    "src-tauri/src/acp/bindings/claude/sdk/probe.rs",
    "Bounded Claude Code model probe",
  ],
  [
    "src-tauri/src/acp/bindings/codex/discovery/probe.rs",
    "Bounded Codex model probe",
  ],
  [
    "src-tauri/src/acp/bindings/pi/title_launch.rs",
    "Admitted Pi title launch with the main session's configuration",
  ],
  [
    "src-tauri/src/acp/bindings/pi/title_request.rs",
    "Bounded no-session Pi title process with tree cleanup",
  ],
  [
    "src-tauri/src/acp/fixture/process_ops.rs",
    "Feature-gated adversarial process fixture",
  ],
  [
    "src-tauri/src/acp/fixture/host/redaction_observer.rs",
    "Feature-gated redaction observer",
  ],
  [
    "src-tauri/src/browser/devtools/cdp_lane/chromium.rs",
    "Feature-gated Chromium measurement lane",
  ],
  [
    "crates/sage-test-support/src/test_executable.rs",
    "Test-only executable fixture writer",
  ],
  [
    "src-tauri/src/console_window_fixture.rs",
    "Test-only console window fixture",
  ],
]);

const frontendSecurityPatterns = [
  [/\bdangerouslySetInnerHTML\b/u, "dangerouslySetInnerHTML"],
  [/\.\s*innerHTML\s*=(?!=)/u, "innerHTML assignment"],
  [/\beval\s*\(/u, "eval"],
  [/\bnew\s+Function\s*\(/u, "Function constructor"],
];
const rustProcessPattern =
  /\bCommand\s*::\s*new\s*\(|\bstd\s*::\s*process\s*::\s*Command\b/u;
// Rust sources of the app crate and of every workspace crate.
const rustSourcePath = /^(?:src-tauri|crates\/[^/]+)\/src\/.+\.rs$/u;
const testSourcePath =
  /(?:^|\/)tests(?:\/|\.rs$)|_tests\.rs$|\.test\.[cm]?[jt]sx?$/u;

const rustFlatToken =
  /r(#+)?"[\s\S]*?"\1|"(?:\\[\s\S]|[^"\\])*"|'(?:\\.|[^'\\])'|\/\/[^\n]*/uy;
const rustBlockCommentOpen = "/*";
const rustBlockCommentClose = "*/";

function blankRustToken(token) {
  return token.replace(/[^\n]/gu, " ");
}

// Returns the end offset of the block comment opening at `start`. Rust block
// comments nest, so each `/*` needs its own `*/`. An unterminated comment
// runs to the end of the source.
function rustBlockCommentEnd(source, start) {
  let depth = 0;
  let index = start;
  while (index < source.length) {
    if (source.startsWith(rustBlockCommentOpen, index)) {
      depth += 1;
      index += rustBlockCommentOpen.length;
    } else if (source.startsWith(rustBlockCommentClose, index)) {
      depth -= 1;
      index += rustBlockCommentClose.length;
      if (depth === 0) return index;
    } else {
      index += 1;
    }
  }
  return source.length;
}

function maskRustLiteralsAndComments(source) {
  let masked = "";
  let index = 0;
  while (index < source.length) {
    if (source.startsWith(rustBlockCommentOpen, index)) {
      const end = rustBlockCommentEnd(source, index);
      masked += blankRustToken(source.slice(index, end));
      index = end;
      continue;
    }
    rustFlatToken.lastIndex = index;
    const token = rustFlatToken.exec(source)?.[0];
    if (token) {
      masked += blankRustToken(token);
      index += token.length;
    } else {
      masked += source[index];
      index += 1;
    }
  }
  return masked;
}

function rustProductionSource(source) {
  // Mask literals and comments before balancing test-module braces. Preserve
  // offsets so a test module cannot hide production code that follows it.
  const masked = maskRustLiteralsAndComments(source);
  const testModule =
    /#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{/gu;
  let result = "";
  let previousEnd = 0;
  for (const match of masked.matchAll(testModule)) {
    if (match.index < previousEnd) continue;
    let end = match.index + match[0].length;
    let depth = 1;
    while (end < masked.length && depth > 0) {
      if (masked[end] === "{") depth += 1;
      if (masked[end] === "}") depth -= 1;
      end += 1;
    }
    result += masked.slice(previousEnd, match.index);
    previousEnd = end;
  }
  return result + masked.slice(previousEnd);
}

export function sourceSecurityErrors(file, source) {
  const relative = canonicalRelative(file);
  if (testSourcePath.test(relative)) return [];
  if (relative.startsWith("src/")) {
    return frontendSecurityPatterns
      .filter(([pattern]) => pattern.test(source))
      .map(
        ([, description]) => `${relative} contains forbidden ${description}.`,
      );
  }
  if (
    rustSourcePath.test(relative) &&
    !reviewedSpawnModules.has(relative) &&
    rustProcessPattern.test(rustProductionSource(source))
  ) {
    return [`${relative} creates processes outside a reviewed spawn module.`];
  }
  return [];
}

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

const tauriConfig = await readJson("src-tauri/tauri.conf.json");
const tauriWindowsConfig = await readJson("src-tauri/tauri.windows.conf.json");
const contractDirectory = "src-tauri/testdata/acp/contracts";
const fixtureVectors = await readJson(
  `${contractDirectory}/fixture-vectors-v1.json`,
);
const packageCargoToml = await readFile(
  path.join(root, "src-tauri/Cargo.toml"),
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

for (const [label, policy] of [
  ["Production", csp],
  ["Development", devCsp],
]) {
  for (const directive of ["object-src", "base-uri", "form-action"]) {
    if (policy?.[directive] !== "'none'") {
      fail(`${label} CSP must set ${directive} to 'none'.`);
    }
  }
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

for (const error of await packageResourcePolicyErrors({
  tauriRoot: path.join(root, "src-tauri"),
  bundle: tauriConfig.bundle,
  windowsBundle,
})) {
  fail(error);
}

const cargoBinBlocks =
  packageCargoToml.match(
    /\[\[bin\]\][\s\S]*?(?=\r?\n\[(?:\[bin\]|features|profile\.)|$)/g,
  ) ?? [];
const helperBinBlocks = cargoBinBlocks.filter((block) =>
  /^name = "sage-codex-elevated-helper"$/m.test(block),
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
  "authenticate_helper_request",
  "execute_authenticated_request",
]) {
  if (helperBoundarySource.includes(forbiddenReference)) {
    fail(
      `The Codex helper boundary must not bypass its narrow managed-publication adapter: ${forbiddenReference}`,
    );
  }
}
if (
  !helperBoundarySource.includes(
    "crate::acp::bindings::codex::execute_boundary_publication",
  )
) {
  fail(
    "The feature-only Codex helper must reach publication only through the narrow authenticated managed-publication adapter.",
  );
}

const helperEntrypointSource = await readFile(
  path.join(root, "src-tauri/src/bin/codex_provisioning_helper.rs"),
  "utf8",
);
if (
  !helperEntrypointSource.includes(
    "sage_lib::codex_helper_boundary::run_helper_process",
  ) ||
  helperEntrypointSource.includes("#[path =")
) {
  fail(
    "The feature-only Codex helper binary must use the library-owned boundary without copying source modules.",
  );
}

for (const error of cargoFeaturePolicyErrors(packageCargoToml)) fail(error);

const capabilities = await readJson("src-tauri/capabilities/main.json");
try {
  const { validateMainCapabilityPermissions } =
    await import("./check-policy-permissions.mjs");
  validateMainCapabilityPermissions(capabilities);
} catch (error) {
  fail(error.message);
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
// External crates are pinned once, in the workspace manifest; members only
// opt in with `.workspace = true` and choose whether a dependency is optional.
const workspaceCargoToml = await readFile(
  path.join(root, "Cargo.toml"),
  "utf8",
);
const crateManifestPaths = (await readdir(path.join(root, "crates")))
  .sort()
  .map((crate) => `crates/${crate}/Cargo.toml`);
const crateManifests = await Promise.all(
  crateManifestPaths.map((file) => readFile(path.join(root, file), "utf8")),
);
const cargoManifests = [cargoToml, workspaceCargoToml, ...crateManifests];
for (const [file, manifest] of [
  ["src-tauri/Cargo.toml", cargoToml],
  ...crateManifestPaths.map((file, index) => [file, crateManifests[index]]),
]) {
  for (const error of memberDependencyErrors(file, manifest)) fail(error);
}
const cargoMetadata = JSON.parse(
  execFileSync(
    "cargo",
    ["metadata", "--format-version", "1", "--all-features", "--locked"],
    {
      cwd: root,
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
    },
  ),
);
for (const error of testSupportGraphErrors(cargoMetadata)) fail(error);
const nativeLibrary = await readFile(
  path.join(root, "src-tauri/src/lib.rs"),
  "utf8",
);
const viteConfig = await readFile(path.join(root, "vite.config.ts"), "utf8");
const acpProtocolContract = await readJson(
  `${contractDirectory}/protocol-contract-v1.json`,
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

if (
  cargoManifests.some((manifest) =>
    /^\s*git2(?:\.workspace)?\s*=/m.test(manifest),
  )
) {
  fail("git2 is forbidden; Sage uses the installed Git CLI.");
}

if (
  cargoManifests.some((manifest) =>
    /^agent-client-protocol(?:\.workspace)?\s*=/mu.test(manifest),
  )
) {
  fail(
    "The ACP runtime crate is forbidden; Sage owns bounded transport and process control.",
  );
}

if (/^acp-runtime\s*=/mu.test(cargoToml)) {
  fail("The rejected acp-runtime feature must not be reachable.");
}

const tracingDeclaration = workspaceCargoToml.match(
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

const windowsDeclaration = workspaceCargoToml.match(
  /^windows-sys\s*=\s*\{([\s\S]*?)^\] \}\s*$/mu,
)?.[1];
const windowsFeatures = [
  "Win32_Foundation",
  "Win32_Security",
  "Win32_Storage_FileSystem",
  "Win32_System_Console",
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

const sourceInventory = await discoverSourceFilesWithCssBound(root);
const securityFiles = [
  ...sourceInventory.files,
  ...(await filesBelow("src-tauri/src")),
  ...(await filesBelow("crates")),
];
for (const file of securityFiles.filter((file) =>
  /\.(?:[cm]?[jt]sx?|rs)$/.test(file),
)) {
  const source = await readFile(path.join(root, file), "utf8");
  for (const error of sourceSecurityErrors(file, source)) fail(error);
}
const productionFiles = securityFiles.filter(
  (file) =>
    /\.(?:ts|tsx|rs)$/.test(file) &&
    !/\.test\.tsx?$/u.test(file) &&
    !/(?:^|[\\/])tests(?:[\\/]|\.rs$)/u.test(file) &&
    !/(?:^|[\\/])[^\\/]+_tests\.rs$/u.test(file),
);

const productionCssFiles = sourceInventory.cssFiles.map(canonicalRelative);
for (const error of await cssHighlightPolicyErrors(root, productionCssFiles)) {
  fail(error);
}
for (const error of await cssHygienePolicyErrors(root, productionCssFiles)) {
  fail(error);
}
for (const error of await cssScalePolicyErrors(root, productionCssFiles)) {
  fail(error);
}
const productionScriptFiles = sourceInventory.files
  .filter((file) => /\.tsx?$/u.test(file) && !/\.test\.tsx?$/u.test(file))
  .map(canonicalRelative);
for (const error of await cssTokenPolicyErrors(
  root,
  productionCssFiles,
  productionScriptFiles,
)) {
  fail(error);
}
for (const error of await cssInlineControlPolicyErrors(
  root,
  productionCssFiles,
)) {
  fail(error);
}

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

const p02LegacySymbols = [
  "BoundedP02ResponseWriter",
  "BlockingP02ResponseWriter",
  "PhysicalWriteControl",
];
for (const file of productionFiles) {
  const source = await readFile(path.join(root, file), "utf8");
  if (p02LegacySymbols.some((symbol) => source.includes(symbol))) {
    fail(`${file} retains a forbidden legacy P-02 blocking delivery symbol.`);
  }
  if (
    file === "src-tauri/src/acp/read_text_file/terminal/delivery.rs" &&
    source.includes("std::io::Write")
  ) {
    fail(
      `${file} must use sealed asynchronous P-02 egress, not std::io::Write.`,
    );
  }
  if (
    (file.startsWith("src-tauri/src/acp/read_text_file/") ||
      file.startsWith("src-tauri/src/acp/transport/")) &&
    source.includes("#![allow(dead_code")
  ) {
    fail(`${file} must not use module-wide dead-code suppression.`);
  }
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

for (const error of await workbenchTransportParityErrors(root)) {
  fail(error);
}
for (const error of await acpTransportParityErrors(root)) {
  fail(error);
}
for (const error of await q02SharedHelperPolicyErrors(root)) {
  fail(error);
}
for (const error of await browserTestPolicyErrors(root)) {
  fail(error);
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

// Every dependency table of a workspace member takes its crate from the
// workspace and may only add `optional`, so the pins, default features and
// feature sets the checks above read from the workspace manifest are what
// builds.
function memberDependencyErrors(file, manifest) {
  const errors = [];
  const tables = manifest.matchAll(
    /^\[(?:target\.[^\]]+\.)?(build-|dev-)?dependencies\]\r?\n([\s\S]*?)(?=^\[|(?![\s\S]))/gmu,
  );
  for (const [, kind, body] of tables) {
    for (const line of body.split(/\r?\n/u)) {
      if (line.trim() === "" || line.trimStart().startsWith("#")) continue;
      const testSupport =
        /^[A-Za-z0-9_-]+\s*=\s*\{\s*workspace\s*=\s*true\s*,\s*features\s*=\s*\["test-support"\]\s*\}\s*$/u.test(
          line,
        );
      if (
        !(kind === "dev-" && testSupport) &&
        !/^[A-Za-z0-9_-]+(?:\.workspace\s*=\s*true|\s*=\s*\{\s*workspace\s*=\s*true\s*,\s*optional\s*=\s*true\s*\})\s*$/u.test(
          line,
        )
      ) {
        errors.push(
          `${file} must take dependencies from the workspace: ${line.trim()}`,
        );
      }
    }
  }
  return errors;
}
