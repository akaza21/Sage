import assert from "node:assert/strict";
import {
  mkdtemp,
  mkdir,
  readFile,
  rm,
  symlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { webBuildSourceFingerprint } from "./web-build-fingerprint.mjs";
import { verifyWebBundle } from "./verify-web-bundle.mjs";

test("named TerminalDock budget isolates xterm from the core bundle", async () => {
  const root = await candidate({ dynamic: "ordinary dynamic" });
  try {
    await mkdir(path.join(root, "src/workbench"), { recursive: true });
    await writeFile(
      path.join(root, "src/workbench/TerminalDock.tsx"),
      'import "@xterm/xterm";\n',
    );
    await writeFile(
      path.join(root, "src/workbench/TerminalDockShell.tsx"),
      'import("./TerminalDock");\n',
    );
    await writeFile(
      path.join(root, "package.json"),
      JSON.stringify({ dependencies: { "@xterm/xterm": "6.0.0" } }),
    );
    await writeFile(
      path.join(root, "dist/assets/terminal.js"),
      "xterm terminal chunk",
    );
    await writeFile(
      path.join(root, "dist/assets/terminal-shell.js"),
      "terminal tab chrome",
    );
    await writeFile(
      path.join(root, "dist/assets/terminal-port.js"),
      "terminal IPC port",
    );
    await writeFile(path.join(root, "dist/assets/terminal.css"), ".xterm {}");
    const manifest = JSON.parse(
      await readFile(path.join(root, "dist/.vite/manifest.json"), "utf8"),
    );
    manifest["index.html"].dynamicImports.push(
      "src/workbench/TerminalDockShell.tsx",
    );
    manifest["src/workbench/TerminalDockShell.tsx"] = {
      file: "assets/terminal-shell.js",
      src: "src/workbench/TerminalDockShell.tsx",
      isDynamicEntry: true,
      imports: ["_terminal-port.js"],
      dynamicImports: ["src/workbench/TerminalDock.tsx"],
    };
    manifest["src/workbench/TerminalDock.tsx"] = {
      file: "assets/terminal.js",
      src: "src/workbench/TerminalDock.tsx",
      isDynamicEntry: true,
      imports: ["index.html", "_terminal-port.js"],
      css: ["assets/terminal.css"],
    };
    manifest["_terminal-port.js"] = {
      file: "assets/terminal-port.js",
      name: "userTerminalDesktop",
      imports: ["index.html"],
    };
    await writeFile(
      path.join(root, "dist/.vite/manifest.json"),
      JSON.stringify(manifest),
    );
    await writeFile(
      path.join(root, "docs/performance/budgets.json"),
      JSON.stringify({
        bundle: {
          webEntryJsGzipBytes: 1000,
          webInitialStaticClosureGzipBytes: 1000,
          webTotalJsGzipBytes: 1000,
          webTerminalLazyJsGzipBytes: 1000,
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
    const result = verifyWebBundle(root);
    assert.ok(result.terminalLazyGzipBytes > 0);
    assert.equal(
      result.coreTotalGzipBytes,
      result.totalGzipBytes - result.terminalLazyGzipBytes,
    );
    assert.ok(result.coreTotalGzipBytes < result.totalGzipBytes);
    const terminalKey = "src/workbench/TerminalDock.tsx";
    manifest["index.html"].dynamicImports = ["_dynamic.js"];
    manifest._orphan = {
      file: "assets/dynamic.js",
      dynamicImports: [terminalKey],
    };
    await writeFile(
      path.join(root, "dist/.vite/manifest.json"),
      JSON.stringify(manifest),
    );
    assert.throws(() => verifyWebBundle(root), /reachable lazy/i);
    delete manifest._orphan;
    manifest["index.html"].dynamicImports.push(
      "src/workbench/TerminalDockShell.tsx",
    );
    manifest._other = { file: "assets/terminal.js" };
    await writeFile(
      path.join(root, "dist/.vite/manifest.json"),
      JSON.stringify(manifest),
    );
    assert.throws(() => verifyWebBundle(root), /shared/i);
    delete manifest._other;
    manifest["index.html"].imports.push(terminalKey);
    await writeFile(
      path.join(root, "dist/.vite/manifest.json"),
      JSON.stringify(manifest),
    );
    assert.throws(() => verifyWebBundle(root), /initial static closure/i);
    manifest["index.html"].imports.pop();
    manifest["index.html"].imports.push("src/workbench/TerminalDockShell.tsx");
    await writeFile(
      path.join(root, "dist/.vite/manifest.json"),
      JSON.stringify(manifest),
    );
    assert.throws(() => verifyWebBundle(root), /initial static closure/i);
    manifest["index.html"].imports.pop();
    manifest["index.html"].imports.push("_terminal-port.js");
    await writeFile(
      path.join(root, "dist/.vite/manifest.json"),
      JSON.stringify(manifest),
    );
    assert.throws(() => verifyWebBundle(root), /initial static closure/i);
    manifest["index.html"].imports.pop();
    manifest["_dynamic.js"].imports = ["_terminal-port.js"];
    await writeFile(
      path.join(root, "dist/.vite/manifest.json"),
      JSON.stringify(manifest),
    );
    assert.throws(() => verifyWebBundle(root), /only by terminal chunks/i);
    delete manifest["_dynamic.js"].imports;
    manifest["index.html"].css = ["assets/terminal.css"];
    await writeFile(
      path.join(root, "dist/.vite/manifest.json"),
      JSON.stringify(manifest),
    );
    assert.throws(() => verifyWebBundle(root), /xterm CSS/i);
    delete manifest["index.html"].css;
    await writeFile(
      path.join(root, "dist/.vite/manifest.json"),
      JSON.stringify(manifest),
    );
    await writeFile(
      path.join(root, "dist/assets/dynamic.js"),
      "xterm in a second chunk",
    );
    assert.throws(() => verifyWebBundle(root), /xterm.*outside.*TerminalDock/i);
    await writeFile(
      path.join(root, "dist/assets/dynamic.js"),
      "ordinary dynamic",
    );
    await writeFile(path.join(root, "dist/assets/terminal.js.map"), "{}");
    assert.throws(() => verifyWebBundle(root), /source map/i);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

const desktopTarget = "desktop-native";
const browserFixtureSentinel =
  "Simulated browser fixture — not live repository state.";

test("named SubagentSurface budget keeps child decoding out of the core", async () => {
  const root = await candidate();
  try {
    const manifestPath = path.join(root, "dist/.vite/manifest.json");
    const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
    const key = "src/workbench/SubagentSurface.tsx";
    manifest["index.html"].dynamicImports.push(key);
    manifest[key] = { file: "assets/subagent.js", isDynamicEntry: true };
    await writeFile(
      path.join(root, "dist/assets/subagent.js"),
      "child view decoder",
    );
    await writeFile(manifestPath, JSON.stringify(manifest));
    const budgetPath = path.join(root, "docs/performance/budgets.json");
    const budgets = JSON.parse(await readFile(budgetPath, "utf8"));
    budgets.bundle.webSubagentLazyJsGzipBytes = 1000;
    await writeFile(budgetPath, JSON.stringify(budgets));
    await writeFile(
      path.join(root, "dist/.sage-web-build-receipt.json"),
      JSON.stringify({
        schemaVersion: 2,
        target: desktopTarget,
        gzip: "node:zlib.gzipSync defaults",
        source: webBuildSourceFingerprint(root),
      }),
    );
    const result = verifyWebBundle(root);
    assert.ok(result.subagentLazyGzipBytes > 0);
    assert.equal(
      result.coreTotalGzipBytes,
      result.totalGzipBytes - result.subagentLazyGzipBytes,
    );
    manifest["index.html"].dynamicImports = ["_dynamic.js"];
    await writeFile(manifestPath, JSON.stringify(manifest));
    assert.throws(
      () => verifyWebBundle(root),
      /reachable, noninitial lazy chunk/,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

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
