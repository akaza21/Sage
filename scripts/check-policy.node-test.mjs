import assert from "node:assert/strict";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { validateMainCapabilityPermissions } from "./check-policy-permissions.mjs";
import {
  curatedPackageResources,
  packageResourcePolicyErrors,
} from "./package-resource-policy.mjs";

test("app commands are allowed only in the exact main webview capability", () => {
  const main = {
    identifier: "main",
    webviews: ["main"],
    permissions: [
      "core:event:allow-listen",
      "core:event:allow-unlisten",
      "main-commands",
    ],
  };
  assert.doesNotThrow(() => validateMainCapabilityPermissions(main));
  for (const capability of [
    { ...main, webviews: ["main", "browser-1"] },
    { ...main, webviews: ["browser-1"] },
    { ...main, windows: ["main"] },
    { ...main, remote: { urls: ["https://example.com"] } },
    { ...main, identifier: "other" },
    { ...main, permissions: ["core:event:allow-listen", "other-command-set"] },
    { ...main, permissions: ["core:default", "main-commands"] },
    { ...main, permissions: ["core:event:allow-emit", "main-commands"] },
  ]) {
    assert.throws(() => validateMainCapabilityPermissions(capability));
  }
});

test("reviewed Windows API features include only ConPTY and existing containment needs", () => {
  const policy = readFileSync(
    new URL("./check-policy.mjs", import.meta.url),
    "utf8",
  );
  const list = policy.match(/const windowsFeatures = \[([\s\S]*?)\];/u)?.[1];
  assert.ok(list, "Windows feature allowlist must exist");
  const features = [...list.matchAll(/"([^"]+)"/gu)].map((match) => match[1]);
  assert.deepEqual(features, [
    "Win32_Foundation",
    "Win32_Security",
    "Win32_Storage_FileSystem",
    "Win32_System_Console",
    "Win32_System_JobObjects",
    "Win32_System_Pipes",
    "Win32_System_Performance",
    "Win32_System_SystemInformation",
    "Win32_System_Threading",
  ]);
});

test("package resources admit only the pinned Pi bridge bytes", async () => {
  const tauriRoot = path.resolve("src-tauri");
  const config = JSON.parse(
    readFileSync(path.join(tauriRoot, "tauri.conf.json"), "utf8"),
  );
  const windows = JSON.parse(
    readFileSync(path.join(tauriRoot, "tauri.windows.conf.json"), "utf8"),
  );
  const check = (overrides) =>
    packageResourcePolicyErrors({
      tauriRoot,
      bundle: config.bundle,
      windowsBundle: windows.bundle,
      ...overrides,
    });
  assert.deepEqual(await check({}), []);

  const [bridge] = curatedPackageResources;
  const resources = [bridge.path];
  for (const bundle of [
    { resources: [...resources, "src/other.mjs"] },
    { resources: [] },
    { resources: { [bridge.path]: "bridge.mjs" } },
    { resources, externalBin: ["bin/helper"] },
  ]) {
    assert.notDeepEqual(await check({ bundle }), [], JSON.stringify(bundle));
  }
  assert.notDeepEqual(
    await check({ windowsBundle: { ...windows.bundle, resources: [] } }),
    [],
  );

  const changed = mkdtempSync(path.join(tmpdir(), "sage-resource-policy-"));
  try {
    const target = path.join(changed, bridge.path);
    mkdirSync(path.dirname(target), { recursive: true });
    const bytes = readFileSync(path.join(tauriRoot, bridge.path));
    bytes[bytes.length - 1] ^= 1;
    writeFileSync(target, bytes);
    assert.deepEqual(await check({ tauriRoot: changed }), [
      `Curated package resource drifted: ${bridge.path}`,
    ]);
    rmSync(target);
    assert.deepEqual(await check({ tauriRoot: changed }), [
      `Curated package resource is missing: ${bridge.path}`,
    ]);
  } finally {
    rmSync(changed, { recursive: true, force: true });
  }
});

const { sourceSecurityErrors } = await import("./check-policy.mjs");

for (const [description, source] of [
  [
    "dangerouslySetInnerHTML",
    "<div dangerouslySetInnerHTML={{ __html: input }} />",
  ],
  ["innerHTML assignment", "element.innerHTML = input;"],
  ["eval", "eval (input);"],
  ["Function constructor", "new Function (input);"],
]) {
  test(`source security rejects ${description}`, () => {
    assert.equal(sourceSecurityErrors("src/example.tsx", source).length, 1);
  });
}

test("source security accepts safe rendering and test fixtures", () => {
  assert.deepEqual(
    sourceSecurityErrors("src/example.tsx", "element.textContent = input;"),
    [],
  );
  assert.deepEqual(
    sourceSecurityErrors("src/example.test.tsx", 'element.innerHTML = "";'),
    [],
  );
});

test("process creation requires an exact reviewed module", () => {
  for (const source of [
    'Command::new("git")',
    'std::process::Command::new("git")',
    "use std::process::Command;",
  ]) {
    assert.equal(
      sourceSecurityErrors("src-tauri/src/unreviewed.rs", source).length,
      1,
    );
    assert.deepEqual(
      sourceSecurityErrors("src-tauri/src/git_process.rs", source),
      [],
    );
    assert.equal(
      sourceSecurityErrors("src-tauri/src/git_process/unreviewed.rs", source)
        .length,
      1,
    );
  }
});

test("Rust test modules do not hide later production process creation", () => {
  const tests =
    '#[cfg(test)]\nmod arbitrary_name { fn check() { let text = "}"; Command::new("git"); } }';
  assert.deepEqual(sourceSecurityErrors("src-tauri/src/example.rs", tests), []);
  assert.equal(
    sourceSecurityErrors(
      "src-tauri/src/example.rs",
      `${tests}\nfn run() { Command::new("git"); }`,
    ).length,
    1,
  );
  for (const file of ["example_tests.rs", "tests.rs", "tests/example.rs"]) {
    assert.deepEqual(
      sourceSecurityErrors(`src-tauri/src/${file}`, 'Command::new("git")'),
      [],
    );
  }
});

test("nested block comments do not hide later production process creation", () => {
  const tests = "#[cfg(test)]\nmod tests { /* outer /* inner */ { */ }";
  assert.deepEqual(sourceSecurityErrors("src-tauri/src/example.rs", tests), []);
  assert.deepEqual(
    sourceSecurityErrors(
      "src-tauri/src/example.rs",
      `${tests}\nfn run() { Command::new("git"); }`,
    ),
    [
      "src-tauri/src/example.rs creates processes outside a reviewed spawn module.",
    ],
  );
  assert.deepEqual(
    sourceSecurityErrors(
      "src-tauri/src/example.rs",
      '/* outer /* inner */ Command::new("git"); */\nfn run() {}',
    ),
    [],
  );
});

test("source security accepts the current source tree", () => {
  const errors = [];
  for (const directory of ["src", "src-tauri/src", "crates"]) {
    for (const file of readdirSync(directory, { recursive: true })) {
      if (!/\.(?:[cm]?[jt]sx?|rs)$/.test(file)) continue;
      const relative = path.join(directory, file);
      errors.push(
        ...sourceSecurityErrors(relative, readFileSync(relative, "utf8")),
      );
    }
  }
  assert.deepEqual(errors, []);
});
