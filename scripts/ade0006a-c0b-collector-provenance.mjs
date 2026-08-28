import path from "node:path";

import { readBoundedFile, sha256 } from "./ade0006a-c0b-common.mjs";

const COLLECTOR_RELATIVE_PATHS = Object.freeze({
  "collect-ade0006a-c0b": "scripts/ade0006a-c0b-campaign-launch.mjs",
  "disposable-profile": "scripts/footprint/DisposableProfile.psm1",
  "process-identity": "scripts/footprint/ProcessIdentity.psm1",
  "semantic-uia": "scripts/footprint/SemanticUia.psm1",
  "windows-footprint": "scripts/footprint/WindowsFootprint.psm1",
  "c0b-observation": "scripts/footprint/C0BObservation.psm1",
});

const COLLECTOR_FILE_BYTES = 1_048_576;

function collectorProvenance(frozen) {
  return frozen?.budgets?.measurementContract?.collectorProvenance;
}

function missing() {
  throw new Error("collector-provenance-missing");
}

function resolveMappedFile(sourceRoot, relativePath) {
  if (typeof sourceRoot !== "string" || !path.isAbsolute(sourceRoot)) {
    missing();
  }
  if (
    typeof relativePath !== "string" ||
    path.isAbsolute(relativePath) ||
    relativePath.includes("\\") ||
    relativePath.split("/").includes("..") ||
    relativePath.split("/").includes("")
  ) {
    missing();
  }
  const absolute = path.resolve(sourceRoot, ...relativePath.split("/"));
  const relative = path.relative(sourceRoot, absolute);
  if (
    relative === "" ||
    relative.startsWith("..") ||
    path.isAbsolute(relative)
  ) {
    missing();
  }
  return absolute;
}

function authenticateNamedFile(sourceRoot, name) {
  const relativePath = COLLECTOR_RELATIVE_PATHS[name];
  if (typeof name !== "string" || !relativePath) missing();
  try {
    const bytes = readBoundedFile(
      resolveMappedFile(sourceRoot, relativePath),
      COLLECTOR_FILE_BYTES,
      name,
    );
    return Object.freeze({
      name,
      sha256: sha256(bytes),
      path: relativePath,
    });
  } catch {
    missing();
  }
}

export function authenticateCollectorProvenance({ sourceRoot, frozen }) {
  const contract = collectorProvenance(frozen);
  if (
    typeof contract?.entrypoint !== "string" ||
    !Array.isArray(contract.modules)
  ) {
    missing();
  }
  return Object.freeze({
    entrypoint: authenticateNamedFile(sourceRoot, contract.entrypoint),
    modules: Object.freeze(
      contract.modules.map((name) => authenticateNamedFile(sourceRoot, name)),
    ),
  });
}

function namedDigest(value) {
  return `${value?.name ?? ""}\0${value?.sha256 ?? ""}`;
}

export function assertCollectorProvenanceMatchesDisk({
  collector,
  sourceRoot,
  frozen,
}) {
  const authenticated = authenticateCollectorProvenance({ sourceRoot, frozen });
  const contract = collectorProvenance(frozen);
  if (
    namedDigest(collector?.entrypoint) !==
      namedDigest(authenticated.entrypoint) ||
    collector?.entrypoint?.name !== contract?.entrypoint ||
    !Array.isArray(collector?.modules) ||
    collector.modules.length !== authenticated.modules.length ||
    collector.modules.map(namedDigest).join("\n") !==
      authenticated.modules.map(namedDigest).join("\n") ||
    collector.modules.map(({ name }) => name).join("\0") !==
      contract.modules.join("\0")
  ) {
    throw new Error("collector-provenance-mismatch");
  }
}

const TOOL_BASENAMES = Object.freeze({
  powershell: Object.freeze([
    "powershell.exe",
    "pwsh.exe",
    "powershell",
    "pwsh",
  ]),
  node: Object.freeze(["node.exe", "node"]),
  git: Object.freeze(["git.exe", "git"]),
});

const TOOL_VERSION = /^[a-z0-9][a-z0-9.+_-]{0,63}$/i;

function toolMissing() {
  throw new Error("collector-tool-missing");
}

function authenticateNamedTool(name, tool, maximumBytes) {
  const allowed = TOOL_BASENAMES[name];
  if (
    !allowed ||
    typeof tool?.path !== "string" ||
    !path.isAbsolute(tool.path) ||
    !TOOL_VERSION.test(tool?.version ?? "")
  ) {
    toolMissing();
  }
  const basename = path.basename(tool.path).toLowerCase();
  if (!allowed.includes(basename)) toolMissing();
  try {
    const bytes = readBoundedFile(tool.path, maximumBytes, name);
    return Object.freeze({
      name,
      version: tool.version,
      sha256: sha256(bytes),
    });
  } catch {
    toolMissing();
  }
}

export function authenticateCollectorTools({ frozen, tools }) {
  const contract = collectorProvenance(frozen);
  const maximumBytes = frozen?.package?.observerBounds?.executableArtifactBytes;
  if (
    !Array.isArray(contract?.tools) ||
    !Number.isSafeInteger(maximumBytes) ||
    maximumBytes < 1
  ) {
    toolMissing();
  }
  return Object.freeze(
    contract.tools.map((name) =>
      authenticateNamedTool(name, tools?.[name], maximumBytes),
    ),
  );
}

function toolDigest(value) {
  return `${value?.name ?? ""}\0${value?.version ?? ""}\0${value?.sha256 ?? ""}`;
}

export function assertCollectorToolsMatchDisk({ collector, frozen, tools }) {
  const authenticated = authenticateCollectorTools({ frozen, tools });
  const contract = collectorProvenance(frozen);
  if (
    !Array.isArray(collector?.tools) ||
    collector.tools.length !== authenticated.length ||
    collector.tools.map(toolDigest).join("\n") !==
      authenticated.map(toolDigest).join("\n") ||
    collector.tools.map(({ name }) => name).join("\0") !==
      contract.tools.join("\0")
  ) {
    throw new Error("collector-tool-mismatch");
  }
}
