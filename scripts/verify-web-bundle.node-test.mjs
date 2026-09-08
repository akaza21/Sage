import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { webBuildSourceFingerprint } from "./web-build-fingerprint.mjs";
import { verifyWebBundle } from "./verify-web-bundle.mjs";

const desktopTarget = "desktop-native";
const browserFixtureSentinel =
  "Simulated browser fixture — not live repository state.";

async function candidate({
  entry = "a",
  imported = "b",
  dynamic = "c",
  limits = [1000, 1000, 2000],
  target = desktopTarget,
} = {}) {
  const root = await mkdtemp(path.join(tmpdir(), "sage-q04-bundle-"));
  await mkdir(path.join(root, "src"), { recursive: true });
  await mkdir(path.join(root, "scripts"), { recursive: true });
  await mkdir(path.join(root, "docs/performance"), { recursive: true });
  await mkdir(path.join(root, "dist/.vite"), { recursive: true });
  await mkdir(path.join(root, "dist/assets"), { recursive: true });
  await writeFile(path.join(root, "src/main.ts"), "export {};\n");
  await writeFile(path.join(root, "scripts/tool.mjs"), "export {};\n");
  await writeFile(path.join(root, "index.html"), '<div id="root"></div>\n');
  await writeFile(path.join(root, "package.json"), "{}\n");
  await writeFile(path.join(root, "package-lock.json"), "{}\n");
  await writeFile(path.join(root, "vite.config.ts"), "export {};\n");
  await writeFile(path.join(root, "tsconfig.json"), "{}\n");
  await writeFile(
    path.join(root, "docs/performance/budgets.json"),
    JSON.stringify({
      bundle: {
        webEntryJsGzipBytes: limits[0],
        webInitialStaticClosureGzipBytes: limits[1],
        webTotalJsGzipBytes: limits[2],
      },
    }),
  );
  await writeFile(path.join(root, "dist/assets/entry.js"), entry);
  await writeFile(path.join(root, "dist/assets/imported.js"), imported);
  await writeFile(path.join(root, "dist/assets/dynamic.js"), dynamic);
  const manifest = {
    "index.html": {
      file: "assets/entry.js",
      src: "index.html",
      isEntry: true,
      imports: ["_imported.js"],
      dynamicImports: ["_dynamic.js"],
    },
    "_imported.js": { file: "assets/imported.js" },
    "_dynamic.js": { file: "assets/dynamic.js" },
  };
  await writeFile(
    path.join(root, "dist/.vite/manifest.json"),
    JSON.stringify(manifest),
  );
  await writeFile(
    path.join(root, "dist/.sage-web-build-target.json"),
    JSON.stringify({ schemaVersion: 1, target: desktopTarget }),
  );
  await writeFile(
    path.join(root, "dist/.sage-web-build-receipt.json"),
    JSON.stringify({
      schemaVersion: 2,
      target,
      gzip: "node:zlib.gzipSync defaults",
      source: webBuildSourceFingerprint(root),
    }),
  );
  return root;
}

test("web bundle verifier counts static closure but includes dynamic chunks in total", async () => {
  const root = await candidate();
  try {
    const result = verifyWebBundle(root);
    assert.deepEqual(result.initialFiles, [
      "assets/entry.js",
      "assets/imported.js",
    ]);
    assert.equal(result.jsFiles.length, 3);
    assert.ok(result.totalGzipBytes > result.initialStaticClosureGzipBytes);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("web bundle verifier enforces exact and plus-one entry, closure, and total limits", async () => {
  const root = await candidate({
    entry: "a".repeat(500),
    imported: "b",
    dynamic: "c",
  });
  try {
    const actual = verifyWebBundle(root);
    await writeFile(
      path.join(root, "docs/performance/budgets.json"),
      JSON.stringify({
        bundle: {
          webEntryJsGzipBytes: actual.entryGzipBytes,
          webInitialStaticClosureGzipBytes:
            actual.initialStaticClosureGzipBytes,
          webTotalJsGzipBytes: actual.totalGzipBytes,
        },
      }),
    );
    await writeFile(
      path.join(root, "dist/.sage-web-build-receipt.json"),
      JSON.stringify({
        schemaVersion: 2,
        target: desktopTarget,
        gzip: "node:zlib.gzipSync defaults",
        source: webBuildSourceFingerprint(root),
      }),
    );
    assert.equal(verifyWebBundle(root).entryGzipBytes, actual.entryGzipBytes);
    await writeFile(
      path.join(root, "docs/performance/budgets.json"),
      JSON.stringify({
        bundle: {
          webEntryJsGzipBytes: actual.entryGzipBytes - 1,
          webInitialStaticClosureGzipBytes:
            actual.initialStaticClosureGzipBytes,
          webTotalJsGzipBytes: actual.totalGzipBytes,
        },
      }),
    );
    await writeFile(
      path.join(root, "dist/.sage-web-build-receipt.json"),
      JSON.stringify({
        schemaVersion: 2,
        target: desktopTarget,
        gzip: "node:zlib.gzipSync defaults",
        source: webBuildSourceFingerprint(root),
      }),
    );
    assert.throws(() => verifyWebBundle(root), /entry gzip/);
    await writeFile(
      path.join(root, "docs/performance/budgets.json"),
      JSON.stringify({
        bundle: {
          webEntryJsGzipBytes: actual.entryGzipBytes,
          webInitialStaticClosureGzipBytes:
            actual.initialStaticClosureGzipBytes - 1,
          webTotalJsGzipBytes: actual.totalGzipBytes,
        },
      }),
    );
    await writeFile(
      path.join(root, "dist/.sage-web-build-receipt.json"),
      JSON.stringify({
        schemaVersion: 2,
        target: desktopTarget,
        gzip: "node:zlib.gzipSync defaults",
        source: webBuildSourceFingerprint(root),
      }),
    );
    assert.throws(() => verifyWebBundle(root), /initial static closure gzip/);
    await writeFile(
      path.join(root, "docs/performance/budgets.json"),
      JSON.stringify({
        bundle: {
          webEntryJsGzipBytes: actual.entryGzipBytes,
          webInitialStaticClosureGzipBytes:
            actual.initialStaticClosureGzipBytes,
          webTotalJsGzipBytes: actual.totalGzipBytes - 1,
        },
      }),
    );
    await writeFile(
      path.join(root, "dist/.sage-web-build-receipt.json"),
      JSON.stringify({
        schemaVersion: 2,
        target: desktopTarget,
        gzip: "node:zlib.gzipSync defaults",
        source: webBuildSourceFingerprint(root),
      }),
    );
    assert.throws(() => verifyWebBundle(root), /total gzip/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("web bundle verifier rejects missing, malformed, stale, and escaping build artifacts", async () => {
  const root = await candidate();
  try {
    await rm(path.join(root, "dist/.vite/manifest.json"));
    assert.throws(
      () => verifyWebBundle(root),
      /manifest is missing or malformed/,
    );
    await writeFile(path.join(root, "dist/.vite/manifest.json"), "{}");
    assert.throws(() => verifyWebBundle(root), /exactly one index.html entry/);
    await writeFile(
      path.join(root, "dist/.vite/manifest.json"),
      JSON.stringify({
        "index.html": {
          file: "../escape.js",
          src: "index.html",
          isEntry: true,
        },
      }),
    );
    assert.throws(() => verifyWebBundle(root), /escaping asset path/);
    await writeFile(
      path.join(root, "dist/.vite/manifest.json"),
      JSON.stringify({
        "index.html": {
          file: "assets/entry.js",
          src: "index.html",
          isEntry: true,
        },
      }),
    );
    await writeFile(path.join(root, "dist/assets/entry.js"), "");
    assert.throws(() => verifyWebBundle(root), /asset is empty/);
    await writeFile(path.join(root, "dist/assets/entry.js"), "a");
    await writeFile(
      path.join(root, "src/main.ts"),
      "export const changed = true;\n",
    );
    assert.throws(() => verifyWebBundle(root), /receipt is stale/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("web bundle verifier rejects a receipt made stale by the HTML entry or lockfile", async () => {
  const htmlRoot = await candidate();
  try {
    await writeFile(
      path.join(htmlRoot, "index.html"),
      "<main>changed</main>\n",
    );
    assert.throws(() => verifyWebBundle(htmlRoot), /receipt is stale/);
  } finally {
    await rm(htmlRoot, { recursive: true, force: true });
  }
  const lockRoot = await candidate();
  try {
    await writeFile(
      path.join(lockRoot, "package-lock.json"),
      '{"lockfileVersion":3}\n',
    );
    assert.throws(() => verifyWebBundle(lockRoot), /receipt is stale/);
  } finally {
    await rm(lockRoot, { recursive: true, force: true });
  }
});

test("web bundle verifier rejects a non-desktop receipt and browser fixture output", async () => {
  const wrongTarget = await candidate({ target: "browser-fixture" });
  try {
    assert.throws(() => verifyWebBundle(wrongTarget), /stale or invalid/);
  } finally {
    await rm(wrongTarget, { recursive: true, force: true });
  }
  const root = await candidate();
  try {
    await writeFile(
      path.join(root, "dist/.sage-web-build-target.json"),
      JSON.stringify({ schemaVersion: 1, target: "browser-fixture" }),
    );
    assert.throws(
      () => verifyWebBundle(root),
      /desktop build target evidence is invalid/,
    );
    await writeFile(
      path.join(root, "dist/.sage-web-build-target.json"),
      JSON.stringify({ schemaVersion: 1, target: desktopTarget }),
    );
    await writeFile(
      path.join(root, "dist/assets/dynamic.js"),
      browserFixtureSentinel,
    );
    assert.throws(() => verifyWebBundle(root), /browser fixture sentinel/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("source fingerprint traversal has exact limits and rejects the next file, depth, byte, and nonregular path", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-q04-fingerprint-"));
  try {
    await mkdir(path.join(root, "src"));
    await writeFile(path.join(root, "src/one.ts"), "a");
    const options = {
      sourceRoots: ["src"],
      limits: { maxFiles: 1, maxDepth: 0, maxBytes: 1 },
    };
    assert.equal(webBuildSourceFingerprint(root, options).files, 1);
    await writeFile(path.join(root, "src/two.ts"), "b");
    assert.throws(
      () => webBuildSourceFingerprint(root, options),
      /maximum file count/,
    );
    await rm(path.join(root, "src/two.ts"));
    await mkdir(path.join(root, "src/nested"));
    assert.throws(
      () => webBuildSourceFingerprint(root, options),
      /maximum depth/,
    );
    await rm(path.join(root, "src/nested"), { recursive: true });
    await writeFile(path.join(root, "src/one.ts"), "ab");
    assert.throws(
      () => webBuildSourceFingerprint(root, options),
      /maximum byte count/,
    );
    await writeFile(path.join(root, "src/one.ts"), "a");
    await symlink("one.ts", path.join(root, "src/link.ts"));
    assert.throws(
      () => webBuildSourceFingerprint(root, options),
      /nonregular source path/,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
