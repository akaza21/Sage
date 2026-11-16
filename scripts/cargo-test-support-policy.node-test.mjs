import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { testSupportGraphErrors } from "./cargo-test-support-policy.mjs";

const root = "/repo";

function dependency(name, overrides = {}) {
  return {
    name,
    rename: null,
    kind: null,
    optional: false,
    features: [],
    target: null,
    ...overrides,
  };
}

function workspace(workspaceRoot, pkg) {
  return {
    workspace_root: workspaceRoot,
    workspace_members: [pkg.name],
    packages: [{ id: pkg.name, source: null, ...pkg }],
    resolve: { nodes: [{ id: pkg.name, deps: [], features: [] }] },
  };
}

function metadata(crate, { dependencies, features = {} }) {
  return workspace(root, {
    name: crate,
    manifest_path: `${root}/crates/${crate}/Cargo.toml`,
    dependencies,
    features,
  });
}

const optionalHelper = dependency("sage-test-support", { optional: true });
const gitTestSupport = {
  default: [],
  "test-support": ["dep:sage-test-support", "sage-workspace/test-support"],
};

test("sage-git may take sage-test-support for its own test-support feature", () => {
  assert.deepEqual(
    testSupportGraphErrors(
      metadata("sage-git", {
        dependencies: [
          optionalHelper,
          dependency("sage-test-support", { kind: "dev" }),
          dependency("sage-workspace", {
            kind: "dev",
            features: ["test-support"],
          }),
        ],
        features: gitTestSupport,
      }),
    ),
    [],
  );
});

test("the allowed manifest matches a Windows workspace root", () => {
  assert.deepEqual(
    testSupportGraphErrors(
      workspace("D:\\Sage", {
        name: "sage-git",
        manifest_path: "D:\\Sage\\crates\\sage-git\\Cargo.toml",
        dependencies: [optionalHelper],
        features: { "test-support": ["dep:sage-test-support"] },
      }),
    ),
    [],
  );
});

const helperOnlyAsDev = "may take sage-test-support only as a dev-dependency";

for (const [description, crate, dependencies, features] of [
  [
    "the same dependency in another crate",
    "sage-core",
    [optionalHelper],
    gitTestSupport,
  ],
  [
    "a required dependency",
    "sage-git",
    [dependency("sage-test-support")],
    gitTestSupport,
  ],
  [
    "a build dependency",
    "sage-git",
    [dependency("sage-test-support", { kind: "build", optional: true })],
    gitTestSupport,
  ],
  [
    "a target-specific dependency",
    "sage-git",
    [
      dependency("sage-test-support", {
        optional: true,
        target: "cfg(unix)",
      }),
    ],
    gitTestSupport,
  ],
  [
    "an optional dependency its test-support feature does not name",
    "sage-git",
    [optionalHelper],
    { "test-support": [] },
  ],
  [
    "a renamed dependency in another crate",
    "sage-core",
    [
      dependency("sage-test-support", {
        rename: "helper",
        optional: true,
      }),
    ],
    { "test-support": ["dep:helper"] },
  ],
]) {
  test(`sage-test-support is rejected as ${description}`, () => {
    assert.ok(
      testSupportGraphErrors(metadata(crate, { dependencies, features })).some(
        (error) => error.includes(helperOnlyAsDev),
      ),
    );
  });
}

for (const [description, features, expected] of [
  [
    "enabled from default",
    { ...gitTestSupport, default: ["dep:sage-test-support"] },
    "sage-git's default feature may not enable dep:sage-test-support.",
  ],
  [
    "behind an implicit feature",
    { ...gitTestSupport, "sage-test-support": ["dep:sage-test-support"] },
    "sage-git's sage-test-support feature may not enable dep:sage-test-support.",
  ],
  [
    "when another feature forwards to it",
    { ...gitTestSupport, fixture: ["sage-test-support/serial"] },
    "sage-git's fixture feature may not enable sage-test-support/serial.",
  ],
]) {
  test(`sage-git's optional sage-test-support is rejected ${description}`, () => {
    assert.deepEqual(
      testSupportGraphErrors(
        metadata("sage-git", { dependencies: [optionalHelper], features }),
      ),
      [expected],
    );
  });
}

test("a non-dev dependency may not enable a test-support feature", () => {
  assert.deepEqual(
    testSupportGraphErrors(
      metadata("sage-db", {
        dependencies: [
          dependency("sage-git", { features: ["test-support"] }),
          dependency("sage-workspace", {
            kind: "build",
            features: ["test-support"],
          }),
        ],
      }),
    ),
    [
      "sage-db may enable sage-git's test-support feature only from [dev-dependencies].",
      "sage-db may enable sage-workspace's test-support feature only from [dev-dependencies].",
    ],
  );
});

for (const value of [
  "sage-git/test-support",
  "sage-git?/test-support",
  "test-support",
]) {
  test(`a feature other than test-support may not enable ${value}`, () => {
    assert.deepEqual(
      testSupportGraphErrors(
        metadata("sage-db", {
          dependencies: [dependency("sage-git", { optional: true })],
          features: { fixture: [value], "test-support": [] },
        }),
      ),
      [`sage-db's fixture feature may not enable ${value}.`],
    );
  });
}

test("a test-support feature forwards only to other test-support features", () => {
  assert.deepEqual(
    testSupportGraphErrors(
      metadata("sage-db", {
        dependencies: [dependency("sage-git", { optional: true })],
        features: {
          "test-support": [
            "sage-git?/test-support",
            "dep:sage-git",
            "sage-git/serial",
          ],
        },
      }),
    ),
    [
      "sage-db's test-support feature may not enable dep:sage-git.",
      "sage-db's test-support feature may not enable sage-git/serial.",
    ],
  );
});

test("the workspace's own cargo metadata passes", () => {
  const resolved = JSON.parse(
    execFileSync(
      "cargo",
      ["metadata", "--format-version", "1", "--all-features", "--locked"],
      {
        encoding: "utf8",
        maxBuffer: 64 * 1024 * 1024,
      },
    ),
  );
  assert.ok(resolved.packages.some(({ name }) => name === "sage-git"));
  assert.deepEqual(testSupportGraphErrors(resolved), []);
});

const manifest = (name, sections = "") =>
  `[package]\nname = "${name}"\nversion = "1.0.0"\nedition = "2024"\n${sections}`;
const vendoredRegistry = {
  ".cargo/config.toml": `[source.crates-io]\nreplace-with = "vendored"\n\n[source.vendored]\ndirectory = "vendor"\n`,
  "vendor/hidden/Cargo.toml": manifest("hidden"),
  "vendor/hidden/src/lib.rs": "",
  "vendor/hidden/.cargo-checksum.json": `{"files":{},"package":null}`,
};
const hiddenForwardsTestSupport = {
  "hidden/Cargo.toml": manifest(
    "hidden",
    `\n[dependencies]\nsage-git = { path = "../sage-git", features = ["test-support"] }\n`,
  ),
  "hidden/src/lib.rs": "",
};
const outsideLocalCrate = [
  "hidden is a local crate outside the workspace members.",
  "hidden is outside the workspace members and may not depend on sage-git.",
];

function resolvedFixture({ app, workspaceTables = "", files }) {
  const directory = mkdtempSync(path.join(tmpdir(), "sage-policy-"));
  try {
    for (const [file, contents] of Object.entries({
      "Cargo.toml": `[workspace]\nresolver = "3"\nmembers = ["app", "sage-git"]\nexclude = ["hidden"]\n${workspaceTables}`,
      "app/Cargo.toml": manifest(
        "app",
        `\n[dependencies]\nsage-git = { path = "../sage-git" }\n${app}\n`,
      ),
      "app/src/lib.rs": "",
      "sage-git/Cargo.toml": manifest(
        "sage-git",
        `\n[features]\ntest-support = []\n`,
      ),
      "sage-git/src/lib.rs": "",
      ...files,
    })) {
      mkdirSync(path.dirname(path.join(directory, file)), { recursive: true });
      writeFileSync(path.join(directory, file), contents);
    }
    const resolved = JSON.parse(
      execFileSync(
        "cargo",
        ["metadata", "--format-version", "1", "--all-features", "--offline"],
        { cwd: directory, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
      ),
    );
    const sageGit = resolved.packages.find(({ name }) => name === "sage-git");
    assert.deepEqual(
      resolved.resolve.nodes.find(({ id }) => id === sageGit.id).features,
      ["test-support"],
    );
    return testSupportGraphErrors(resolved);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

for (const [description, fixture, expected] of [
  [
    "a local crate outside the workspace members",
    {
      app: `hidden = { path = "../hidden" }`,
      files: hiddenForwardsTestSupport,
    },
    outsideLocalCrate,
  ],
  [
    "an optional local crate outside the workspace members",
    {
      app: `hidden = { path = "../hidden", optional = true }\n\n[features]\nplugin = ["dep:hidden"]`,
      files: hiddenForwardsTestSupport,
    },
    outsideLocalCrate,
  ],
  [
    "a [patch.crates-io] to a local crate outside the workspace members",
    {
      app: `hidden = "1"`,
      workspaceTables: `\n[patch.crates-io]\nhidden = { path = "hidden" }\n`,
      files: hiddenForwardsTestSupport,
    },
    outsideLocalCrate,
  ],
  [
    "a [replace] with a local crate outside the workspace members",
    {
      app: `hidden = "1"`,
      workspaceTables: `\n[replace]\n"hidden:1.0.0" = { path = "hidden" }\n`,
      files: { ...vendoredRegistry, ...hiddenForwardsTestSupport },
    },
    outsideLocalCrate,
  ],
  [
    "a registry crate reaching a workspace member through [patch.crates-io]",
    {
      app: `hidden = "1"`,
      workspaceTables: `\n[patch.crates-io]\nsage-git = { path = "sage-git" }\n`,
      files: {
        ...vendoredRegistry,
        "vendor/hidden/Cargo.toml": manifest(
          "hidden",
          `\n[dependencies]\nsage-git = { version = "1", features = ["test-support"] }\n`,
        ),
      },
    },
    ["hidden is outside the workspace members and may not depend on sage-git."],
  ],
]) {
  test(`${description} that enables sage-git's test-support fails the check`, () => {
    assert.deepEqual(resolvedFixture(fixture), expected);
  });
}
