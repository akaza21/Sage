import { createHash } from "node:crypto";
import { readFile, readdir } from "node:fs/promises";
import path from "node:path";
import process from "node:process";

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

const ownershipManifest = await readJson(
  "docs/work-items/ADE-0006A-acp-session-foundation/contracts/ownership-manifest-v1.json",
);
const ownershipScope = new Set(ownershipManifest.ownedFileScopes ?? []);
for (const directory of ownershipManifest.ownedDirectoryScopes ?? []) {
  for (const file of await filesBelow(directory)) {
    ownershipScope.add(canonicalRelative(file));
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
const fixtureVectors = await readJson(
  "docs/work-items/ADE-0006A-acp-session-foundation/contracts/fixture-vectors-v1.json",
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
