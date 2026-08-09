import assert from "node:assert/strict";
import {
  linkSync,
  mkdtempSync,
  mkdirSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import {
  collectPackageParticipantProvenance,
  collectPackageSourceIdentity,
} from "./ade0006a-package-provenance.mjs";

function fixture(participant = "candidate") {
  const sourceRoot = mkdtempSync(path.join(tmpdir(), "sage-provenance-"));
  const applicationRoot =
    participant === "candidate" ? sourceRoot : path.join(sourceRoot, "control");
  const cargoRoot = path.join(applicationRoot, "src-tauri");
  const frontendRoot = path.join(applicationRoot, "frontend");
  mkdirSync(cargoRoot, { recursive: true });
  mkdirSync(frontendRoot, { recursive: true });
  writeFileSync(
    path.join(cargoRoot, "Cargo.toml"),
    "[package]\nname='app'\nversion='0.1.0'\n",
  );
  writeFileSync(path.join(cargoRoot, "Cargo.lock"), "cargo-lock\n");
  writeFileSync(path.join(frontendRoot, "index.html"), "front");
  writeFileSync(path.join(frontendRoot, "app.js"), "end");
  const rootId = `app 0.1.0 (path+file://${cargoRoot.replaceAll("\\", "/")})`;
  const dependencyId =
    "dep 1.2.3 (registry+https://github.com/rust-lang/crates.io-index)";
  const metadata = {
    packages: [
      {
        id: rootId,
        name: "app",
        version: "0.1.0",
        source: null,
        checksum: null,
        manifest_path: path.join(cargoRoot, "Cargo.toml"),
      },
      {
        id: dependencyId,
        name: "dep",
        version: "1.2.3",
        source: "registry+https://github.com/rust-lang/crates.io-index",
        checksum: "a".repeat(64),
        manifest_path: path.join(sourceRoot, "registry", "dep", "Cargo.toml"),
      },
    ],
    resolve: {
      root: rootId,
      nodes: [
        {
          id: rootId,
          features: ["default", "custom-protocol"],
          dependencies: [dependencyId],
          deps: [
            {
              pkg: dependencyId,
              dep_kinds: [{ kind: null, target: null }],
            },
          ],
        },
        { id: dependencyId, features: [], dependencies: [], deps: [] },
      ],
    },
  };
  const cargo = process.execPath;
  writeFileSync(
    path.join(sourceRoot, "metadata"),
    `process.stdout.write(${JSON.stringify(JSON.stringify(metadata))});\n`,
  );
  const frontendLock = participant === "candidate" ? "package-lock.json" : null;
  if (frontendLock) {
    writeFileSync(
      path.join(applicationRoot, frontendLock),
      JSON.stringify({
        lockfileVersion: 3,
        packages: {
          "": { name: "app", version: "0.1.0" },
          "node_modules/view": {
            version: "2.0.0",
            resolved: "https://registry.npmjs.org/view/-/view-2.0.0.tgz",
            integrity: "sha512-test",
            dependencies: { leaf: "1.0.0" },
          },
          "node_modules/leaf": {
            version: "1.0.0",
            resolved: "https://registry.npmjs.org/leaf/-/leaf-1.0.0.tgz",
            integrity: "sha512-leaf",
          },
        },
      }),
    );
  }
  const relativeApplicationRoot =
    path.relative(sourceRoot, applicationRoot) || ".";
  const frozen = {
    limits: {
      wire: {
        jsonDepth: 32,
        jsonNodes: 16384,
        objectMembers: 256,
        arrayEntries: 512,
        decodedKeyBytes: 256,
      },
    },
    package: {
      productionBuild: { enabledFeatures: ["custom-protocol"] },
      observerBounds: {
        frontendMembers: 64,
        frontendBytes: 1024,
        pathUtf8Bytes: 256,
        lockfileBytes: 1024 * 1024,
        dependencyGraphBytes: 1024 * 1024,
        dependencyPackages: 64,
        dependencyJsonNodes: 4096,
        toolStdoutBytes: 1024 * 1024,
        toolStderrBytes: 1024,
      },
      participants: {
        [participant]: {
          applicationRoot: relativeApplicationRoot,
          cargoManifest: path.join(
            relativeApplicationRoot,
            "src-tauri",
            "Cargo.toml",
          ),
          provenance: {
            cargoLock: path.join(
              relativeApplicationRoot,
              "src-tauri",
              "Cargo.lock",
            ),
            frontendDependencyMode:
              participant === "candidate"
                ? "npm-package-lock-v3"
                : "static-no-package-manager",
            frontendLock: frontendLock
              ? path.join(relativeApplicationRoot, frontendLock)
              : null,
            frontendDist: path.join(relativeApplicationRoot, "frontend"),
          },
        },
      },
    },
  };
  return {
    sourceRoot,
    frontendRoot,
    cargo,
    frozen,
    participant,
  };
}

async function collect(value) {
  return collectPackageParticipantProvenance({
    sourceRoot: value.sourceRoot,
    participant: value.participant,
    buildTools: {
      cargo: { path: value.cargo },
      rustc: { path: value.cargo },
      createCargoEnvironment: () => process.env,
      metadataTargetDirectory: value.sourceRoot,
    },
    frozen: value.frozen,
  });
}

test("participant provenance is bounded and independent of absolute workspace paths", async () => {
  const first = fixture();
  const second = fixture();
  try {
    const left = await collect(first);
    const right = await collect(second);
    assert.equal(left.frontendRawBytes, 8);
    assert.equal(left.dependencyPackageCount, 4);
    assert.equal(left.dependencyGraph.cargo.length, 2);
    assert.equal(left.dependencyGraph.frontend.length, 2);
    assert.equal(left.frontendMembers.length, 2);
    assert.equal(left.dependencyGraphSha256, right.dependencyGraphSha256);
    assert.match(left.cargoLockSha256, /^[a-f0-9]{64}$/);
    assert.match(left.frontendLockSha256, /^[a-f0-9]{64}$/);
  } finally {
    rmSync(first.sourceRoot, { recursive: true, force: true });
    rmSync(second.sourceRoot, { recursive: true, force: true });
  }
});

test("static control provenance has no synthetic frontend lock", async () => {
  const value = fixture("control");
  try {
    const observed = await collect(value);
    assert.equal(observed.frontendDependencyMode, "static-no-package-manager");
    assert.equal(observed.frontendLockSha256, null);
    assert.equal(observed.dependencyPackageCount, 2);
  } finally {
    rmSync(value.sourceRoot, { recursive: true, force: true });
  }
});

test("frontend provenance rejects symbolic and hard-linked files", async () => {
  for (const kind of ["symbolic", "hard"]) {
    const value = fixture();
    try {
      const target = path.join(value.frontendRoot, "app.js");
      const linked = path.join(value.frontendRoot, `${kind}.js`);
      if (kind === "symbolic") symlinkSync(target, linked);
      else linkSync(target, linked);
      await assert.rejects(collect(value), /linked|symbolic|reparse/);
    } finally {
      rmSync(value.sourceRoot, { recursive: true, force: true });
    }
  }
});

test("source identity comes from the authenticated Git executable", async () => {
  const sourceRoot = mkdtempSync(path.join(tmpdir(), "sage-source-identity-"));
  const git = process.execPath;
  writeFileSync(
    path.join(sourceRoot, "rev-parse"),
    `process.stdout.write(process.argv[3] === "HEAD" ? ${JSON.stringify(
      "a".repeat(40),
    )} : ${JSON.stringify("b".repeat(40))});\n`,
  );
  try {
    assert.deepEqual(
      await collectPackageSourceIdentity({
        sourceRoot,
        buildTools: { git: { path: git }, gitEnvironment: process.env },
      }),
      { commit: "a".repeat(40), tree: "b".repeat(40) },
    );
  } finally {
    rmSync(sourceRoot, { recursive: true, force: true });
  }
});
