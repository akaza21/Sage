import path from "node:path";

import {
  parseBoundedJson,
  recordDigest,
  stableJson,
  validateJsonShape,
} from "./ade0006a-c0b-common.mjs";
import { runBounded } from "./ade0006a-package-build.mjs";
import {
  hashBoundedFile,
  inspectBoundedTreeMembers,
} from "./ade0006a-package-members.mjs";

const cargoMetadataTimeoutMs = 2 * 60 * 1000;
const gitObject = /^(?:[a-f0-9]{40}|[a-f0-9]{64})$/;

function compareCanonical(left, right) {
  return Buffer.compare(
    Buffer.from(stableJson(left)),
    Buffer.from(stableJson(right)),
  );
}

function within(child, parent) {
  const relative = path.relative(parent, child);
  return (
    relative === "" ||
    (!relative.startsWith("..") && !path.isAbsolute(relative))
  );
}

function workspaceSource(sourceRoot, participant, manifestPath) {
  const relative = path.relative(sourceRoot, manifestPath);
  if (!within(manifestPath, sourceRoot) || relative === "") {
    throw new Error("cargo-metadata-workspace-path-invalid");
  }
  return `workspace:${participant}:${relative.split(path.sep).join("/")}`;
}

function cargoDependencies(node) {
  if (!Array.isArray(node?.deps)) {
    throw new Error("cargo-metadata-dependency-edges-invalid");
  }
  return node.deps
    .filter(
      (dependency) =>
        Array.isArray(dependency.dep_kinds) &&
        dependency.dep_kinds.some(({ kind }) => kind !== "dev"),
    )
    .map(({ pkg }) => pkg);
}

function canonicalCargoGraph(
  metadata,
  sourceRoot,
  participant,
  maximumPackages,
) {
  if (
    !Array.isArray(metadata?.packages) ||
    !Array.isArray(metadata?.resolve?.nodes) ||
    typeof metadata?.resolve?.root !== "string"
  ) {
    throw new Error("cargo-metadata-shape-invalid");
  }
  const packages = new Map(metadata.packages.map((value) => [value.id, value]));
  const nodes = new Map(
    metadata.resolve.nodes.map((value) => [value.id, value]),
  );
  const pending = [metadata.resolve.root];
  const reachable = new Set();
  while (pending.length > 0) {
    const id = pending.pop();
    if (reachable.has(id)) continue;
    const node = nodes.get(id);
    const pkg = packages.get(id);
    if (!node || !pkg) throw new Error("cargo-metadata-resolve-invalid");
    reachable.add(id);
    if (reachable.size > maximumPackages) {
      throw new Error("dependency-package-count-oversize");
    }
    pending.push(...cargoDependencies(node));
  }
  const identity = new Map();
  for (const id of reachable) {
    const pkg = packages.get(id);
    if (
      typeof pkg.name !== "string" ||
      typeof pkg.version !== "string" ||
      typeof pkg.manifest_path !== "string"
    ) {
      throw new Error("cargo-metadata-package-invalid");
    }
    const source =
      pkg.source == null
        ? workspaceSource(
            sourceRoot,
            participant,
            path.resolve(pkg.manifest_path),
          )
        : pkg.source;
    if (
      typeof source !== "string" ||
      (pkg.checksum != null && typeof pkg.checksum !== "string")
    ) {
      throw new Error("cargo-metadata-package-identity-invalid");
    }
    identity.set(
      id,
      recordDigest({
        ecosystem: "cargo",
        name: pkg.name,
        version: pkg.version,
        source,
        checksum: pkg.checksum ?? null,
      }),
    );
  }
  return [...reachable]
    .map((id) => {
      const pkg = packages.get(id);
      const node = nodes.get(id);
      const source =
        pkg.source == null
          ? workspaceSource(
              sourceRoot,
              participant,
              path.resolve(pkg.manifest_path),
            )
          : pkg.source;
      const dependencies = cargoDependencies(node).map((dependency) => {
        const key = identity.get(dependency);
        if (!key) throw new Error("cargo-metadata-unreachable-edge-invalid");
        return key;
      });
      return {
        ecosystem: "cargo",
        name: pkg.name,
        version: pkg.version,
        source,
        checksum: pkg.checksum ?? null,
        features: [...(node.features ?? [])].sort(),
        dependencies: [...new Set(dependencies)].sort(),
      };
    })
    .sort(compareCanonical);
}

function npmDependencyPath(packagePath, dependencyName, knownPaths) {
  let segments = packagePath.split("/");
  const nameSegments = dependencyName.split("/");
  while (true) {
    const candidate = [...segments, "node_modules", ...nameSegments].join("/");
    if (knownPaths.has(candidate)) return candidate;
    const lastNodeModules = segments.lastIndexOf("node_modules");
    if (lastNodeModules < 0) break;
    segments = segments.slice(0, lastNodeModules);
  }
  const rootCandidate = ["node_modules", ...nameSegments].join("/");
  if (knownPaths.has(rootCandidate)) return rootCandidate;
  throw new Error("frontend-lock-dependency-unresolved");
}

function packageName(packagePath, value) {
  if (typeof value.name === "string" && value.name.length > 0)
    return value.name;
  const segments = packagePath.split("/");
  const nodeModules = segments.lastIndexOf("node_modules");
  const tail = segments.slice(nodeModules + 1);
  return tail[0]?.startsWith("@") ? `${tail[0]}/${tail[1]}` : tail[0];
}

function canonicalFrontendGraph(lock, maximumPackages) {
  if (
    lock?.lockfileVersion !== 3 ||
    !lock.packages ||
    Array.isArray(lock.packages)
  ) {
    throw new Error("frontend-lock-shape-invalid");
  }
  const entries = Object.entries(lock.packages).filter(
    ([packagePath]) => packagePath !== "",
  );
  if (entries.length > maximumPackages)
    throw new Error("dependency-package-count-oversize");
  const knownPaths = new Set(entries.map(([packagePath]) => packagePath));
  const identities = new Map(
    entries.map(([packagePath, value]) => [
      packagePath,
      recordDigest({
        ecosystem: "npm",
        name: packageName(packagePath, value),
        version: value.version,
        source: value.resolved ?? null,
        checksum: value.integrity ?? null,
      }),
    ]),
  );
  return entries
    .map(([packagePath, value]) => {
      const name = packageName(packagePath, value);
      if (typeof name !== "string" || typeof value.version !== "string") {
        throw new Error("frontend-lock-package-invalid");
      }
      const dependencies = Object.keys(value.dependencies ?? {}).map(
        (dependency) => {
          const resolved = npmDependencyPath(
            packagePath,
            dependency,
            knownPaths,
          );
          return identities.get(resolved);
        },
      );
      return {
        ecosystem: "npm",
        name,
        version: value.version,
        source: value.resolved ?? null,
        checksum: value.integrity ?? null,
        features: [],
        dependencies: [...new Set(dependencies)].sort(),
      };
    })
    .sort(compareCanonical);
}

function cargoEnvironment(buildTools) {
  if (
    typeof buildTools.createCargoEnvironment !== "function" ||
    !path.isAbsolute(buildTools.metadataTargetDirectory ?? "")
  ) {
    throw new Error("authenticated-package-environment-missing");
  }
  return buildTools.createCargoEnvironment(buildTools.metadataTargetDirectory);
}

async function readGitObject(sourceRoot, buildTools, revision, label) {
  const result = await runBounded(
    buildTools.git.path,
    ["rev-parse", "--verify", revision],
    {
      cwd: sourceRoot,
      env: buildTools.gitEnvironment,
      timeoutMs: 30 * 1000,
      label,
      stdoutMaximum: 128,
      stderrMaximum: 1024,
      captureStdout: true,
    },
  );
  const value = result.stdout.toString("utf8").trim();
  if (!gitObject.test(value)) throw new Error(`${label}-object-invalid`);
  return value;
}

export async function collectPackageSourceIdentity({ sourceRoot, buildTools }) {
  const [commit, tree] = await Promise.all([
    readGitObject(sourceRoot, buildTools, "HEAD", "source-commit"),
    readGitObject(sourceRoot, buildTools, "HEAD^{tree}", "source-tree"),
  ]);
  return { commit, tree };
}

async function collectCargoGraph({
  sourceRoot,
  participant,
  profile,
  buildTools,
  frozen,
}) {
  const result = await runBounded(
    buildTools.cargo.path,
    [
      "metadata",
      "--locked",
      "--offline",
      "--format-version",
      "1",
      "--filter-platform",
      "x86_64-pc-windows-msvc",
      "--features",
      frozen.package.productionBuild.enabledFeatures.join(","),
      "--manifest-path",
      path.join(sourceRoot, profile.cargoManifest),
    ],
    {
      cwd: sourceRoot,
      env: cargoEnvironment(buildTools),
      timeoutMs: cargoMetadataTimeoutMs,
      label: `${participant}-cargo-metadata`,
      stdoutMaximum: frozen.package.observerBounds.dependencyGraphBytes,
      stderrMaximum: frozen.package.observerBounds.toolStderrBytes,
      captureStdout: true,
    },
  );
  const metadata = JSON.parse(result.stdout);
  validateJsonShape(
    metadata,
    {
      ...frozen.limits.wire,
      arrayEntries: frozen.package.observerBounds.dependencyPackages,
      objectMembers: frozen.package.observerBounds.dependencyPackages,
      jsonNodes: frozen.package.observerBounds.dependencyJsonNodes,
    },
    `${participant} Cargo metadata`,
  );
  return canonicalCargoGraph(
    metadata,
    sourceRoot,
    participant,
    frozen.package.observerBounds.dependencyPackages,
  );
}

export async function collectPackageParticipantProvenance({
  sourceRoot,
  participant,
  buildTools,
  frozen,
}) {
  const profile = frozen.package.participants[participant];
  const provenance = profile?.provenance;
  if (!profile || !provenance)
    throw new Error("participant-provenance-contract-missing");
  const bounds = frozen.package.observerBounds;
  const cargoLock = await hashBoundedFile(
    path.join(sourceRoot, provenance.cargoLock),
    bounds.lockfileBytes,
    { rejectHardLinks: true },
  );
  let frontendLockSha256 = null;
  let frontend = [];
  if (provenance.frontendDependencyMode === "npm-package-lock-v3") {
    const record = parseBoundedJson(
      path.join(sourceRoot, provenance.frontendLock),
      bounds.lockfileBytes,
      {
        ...frozen.limits.wire,
        arrayEntries: bounds.dependencyPackages,
        objectMembers: bounds.dependencyPackages,
        jsonNodes: bounds.dependencyJsonNodes,
      },
      `${participant} frontend lock`,
    );
    frontendLockSha256 = record.sha256;
    frontend = canonicalFrontendGraph(record.value, bounds.dependencyPackages);
  } else if (
    provenance.frontendDependencyMode !== "static-no-package-manager" ||
    provenance.frontendLock !== null
  ) {
    throw new Error("frontend-dependency-mode-invalid");
  }
  const cargo = await collectCargoGraph({
    sourceRoot,
    participant,
    profile,
    buildTools,
    frozen,
  });
  const frontendMembers = await inspectBoundedTreeMembers(
    path.join(sourceRoot, provenance.frontendDist),
    {
      membersPerInstaller: bounds.frontendMembers,
      extractedBytesPerInstaller: bounds.frontendBytes,
      pathUtf8Bytes: bounds.pathUtf8Bytes,
    },
  );
  const frontendRawBytes = frontendMembers
    .filter(({ type }) => type === "regularFile")
    .reduce((total, { bytes }) => total + bytes, 0);
  const graph = { cargo, frontend };
  const dependencyPackageCount = cargo.length + frontend.length;
  if (
    dependencyPackageCount > bounds.dependencyPackages ||
    Buffer.byteLength(stableJson(graph)) > bounds.dependencyGraphBytes
  ) {
    throw new Error("dependency-graph-bound-exceeded");
  }
  return {
    cargoLockSha256: cargoLock.sha256,
    frontendDependencyMode: provenance.frontendDependencyMode,
    frontendLockSha256,
    dependencyGraphSha256: recordDigest(graph),
    dependencyPackageCount,
    frontendRawBytes,
    frontendTreeSha256: recordDigest(frontendMembers),
    dependencyGraph: graph,
    frontendMembers,
  };
}
