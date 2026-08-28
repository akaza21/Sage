import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  assertCollectorProvenanceMatchesDisk,
  assertCollectorToolsMatchDisk,
  authenticateCollectorProvenance,
  authenticateCollectorTools,
} from "./ade0006a-c0b-collector-provenance.mjs";
import { loadFrozenContracts } from "./ade0006a-c0b-contracts.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const frozen = loadFrozenContracts(root);

const COLLECTOR_PATHS = Object.freeze({
  "collect-ade0006a-c0b": "scripts/ade0006a-c0b-campaign-launch.mjs",
  "disposable-profile": "scripts/footprint/DisposableProfile.psm1",
  "process-identity": "scripts/footprint/ProcessIdentity.psm1",
  "semantic-uia": "scripts/footprint/SemanticUia.psm1",
  "windows-footprint": "scripts/footprint/WindowsFootprint.psm1",
  "c0b-observation": "scripts/footprint/C0BObservation.psm1",
});

function independentSha256(relativePath) {
  return createHash("sha256")
    .update(readFileSync(path.join(root, ...relativePath.split("/"))))
    .digest("hex");
}

function diskCollector() {
  return authenticateCollectorProvenance({ sourceRoot: root, frozen });
}

function nameAndSha256(value) {
  return {
    entrypoint: {
      name: value.entrypoint.name,
      sha256: value.entrypoint.sha256,
    },
    modules: value.modules.map(({ name, sha256 }) => ({ name, sha256 })),
  };
}

function allACollector() {
  const digest = "a".repeat(64);
  const provenance = frozen.budgets.measurementContract.collectorProvenance;
  return {
    entrypoint: { name: provenance.entrypoint, sha256: digest },
    modules: provenance.modules.map((name) => ({ name, sha256: digest })),
  };
}

test("authenticateCollectorProvenance hashes the named collector files from disk", () => {
  const authenticated = diskCollector();
  const provenance = frozen.budgets.measurementContract.collectorProvenance;

  assert.equal(authenticated.entrypoint.name, "collect-ade0006a-c0b");
  assert.equal(authenticated.entrypoint.name, provenance.entrypoint);
  assert.equal(
    authenticated.entrypoint.path,
    COLLECTOR_PATHS["collect-ade0006a-c0b"],
  );
  assert.equal(
    authenticated.entrypoint.sha256,
    independentSha256(COLLECTOR_PATHS["collect-ade0006a-c0b"]),
  );

  assert.deepEqual(
    authenticated.modules.map(({ name }) => name),
    provenance.modules,
  );
  for (const module of authenticated.modules) {
    assert.equal(module.path, COLLECTOR_PATHS[module.name]);
    assert.equal(module.sha256, independentSha256(module.path));
  }
});

test("assertCollectorProvenanceMatchesDisk accepts the authenticate result", () => {
  assert.doesNotThrow(() =>
    assertCollectorProvenanceMatchesDisk({
      collector: nameAndSha256(diskCollector()),
      sourceRoot: root,
      frozen,
    }),
  );
});

test("assertCollectorProvenanceMatchesDisk rejects a swapped or fixture digest", () => {
  const collector = nameAndSha256(diskCollector());

  collector.modules[0] = {
    ...collector.modules[0],
    sha256: "a".repeat(64),
  };
  assert.throws(
    () =>
      assertCollectorProvenanceMatchesDisk({
        collector,
        sourceRoot: root,
        frozen,
      }),
    /collector-provenance-mismatch/,
  );

  assert.throws(
    () =>
      assertCollectorProvenanceMatchesDisk({
        collector: {
          ...nameAndSha256(diskCollector()),
          entrypoint: {
            name: diskCollector().entrypoint.name,
            sha256: "a".repeat(64),
          },
        },
        sourceRoot: root,
        frozen,
      }),
    /collector-provenance-mismatch/,
  );
});

test("assertCollectorProvenanceMatchesDisk rejects a matching fake collector against disk", () => {
  assert.throws(
    () =>
      assertCollectorProvenanceMatchesDisk({
        collector: allACollector(),
        sourceRoot: root,
        frozen,
      }),
    /collector-provenance-mismatch/,
  );
});

test("authenticateCollectorProvenance rejects a sourceRoot that lacks a mapped file", () => {
  const sourceRoot = mkdtempSync(path.join(tmpdir(), "sage-c0b-provenance-"));
  try {
    assert.throws(
      () => authenticateCollectorProvenance({ sourceRoot, frozen }),
      /collector-provenance-missing/,
    );
  } finally {
    rmSync(sourceRoot, { recursive: true, force: true });
  }
});

function requiredToolPath(filePath, label) {
  if (!existsSync(filePath)) {
    throw new Error(`test host missing ${label}: ${filePath}`);
  }
  return filePath;
}

function hostToolPaths() {
  const powershell = requiredToolPath(
    path.join(
      process.env.SystemRoot ?? "C:\\Windows",
      "System32",
      "WindowsPowerShell",
      "v1.0",
      "powershell.exe",
    ),
    "powershell",
  );
  const gitCandidates = [
    "C:\\Program Files\\Git\\cmd\\git.exe",
    "C:\\Program Files\\Git\\bin\\git.exe",
  ];
  const git = gitCandidates.find((candidate) => existsSync(candidate));
  if (!git) throw new Error("test host missing git.exe");
  return {
    powershell: { path: powershell, version: "5.1.0" },
    node: {
      path: requiredToolPath(process.execPath, "node"),
      version: "22.0.0",
    },
    git: { path: git, version: "2.0.0" },
  };
}

function independentFileSha256(filePath) {
  return createHash("sha256").update(readFileSync(filePath)).digest("hex");
}

function nameVersionSha256(tools) {
  return tools.map(({ name, version, sha256 }) => ({ name, version, sha256 }));
}

function allATools() {
  const digest = "a".repeat(64);
  return frozen.budgets.measurementContract.collectorProvenance.tools.map(
    (name, index) => ({
      name,
      version: `${index + 1}.0.0`,
      sha256: digest,
    }),
  );
}

test("authenticateCollectorTools hashes powershell, node, and git from disk", () => {
  const tools = hostToolPaths();
  const authenticated = authenticateCollectorTools({ frozen, tools });
  const names = frozen.budgets.measurementContract.collectorProvenance.tools;

  assert.deepEqual(
    authenticated.map(({ name }) => name),
    names,
  );
  assert.equal(authenticated.length, 3);
  for (const record of authenticated) {
    const host = tools[record.name];
    assert.equal(record.version, host.version);
    assert.equal(record.sha256, independentFileSha256(host.path));
  }
});

test("assertCollectorToolsMatchDisk accepts the authenticate result", () => {
  const tools = hostToolPaths();
  assert.doesNotThrow(() =>
    assertCollectorToolsMatchDisk({
      collector: {
        tools: nameVersionSha256(authenticateCollectorTools({ frozen, tools })),
      },
      frozen,
      tools,
    }),
  );
});

test("assertCollectorToolsMatchDisk rejects a swapped or fixture digest", () => {
  const tools = hostToolPaths();
  const collectorTools = nameVersionSha256(
    authenticateCollectorTools({ frozen, tools }),
  );
  collectorTools[1] = { ...collectorTools[1], sha256: "a".repeat(64) };
  assert.throws(
    () =>
      assertCollectorToolsMatchDisk({
        collector: { tools: collectorTools },
        frozen,
        tools,
      }),
    /collector-tool-mismatch/,
  );
});

test("assertCollectorToolsMatchDisk rejects a matching fake tool set against disk", () => {
  assert.throws(
    () =>
      assertCollectorToolsMatchDisk({
        collector: { tools: allATools() },
        frozen,
        tools: hostToolPaths(),
      }),
    /collector-tool-mismatch/,
  );
});

test("authenticateCollectorTools rejects a path whose basename is not that tool", () => {
  const tools = hostToolPaths();
  assert.throws(
    () =>
      authenticateCollectorTools({
        frozen,
        tools: {
          ...tools,
          powershell: { path: tools.node.path, version: "5.1.0" },
        },
      }),
    /collector-tool-missing/,
  );
});
