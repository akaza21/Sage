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
import { gzipSync } from "node:zlib";
import test from "node:test";

import { webBuildSourceFingerprint } from "./web-build-fingerprint.mjs";
import {
  normalizeHashedImports,
  verifyWebBundle,
} from "./verify-web-bundle.mjs";
import { parseLazyFeatureBudgets } from "./web-bundle-lazy-features.mjs";
import {
  WEB_BUNDLE_PATH_ROOTS,
  measureWebBundlePaths,
} from "./web-bundle-paths.mjs";

const pathLimits = Object.fromEntries(
  Object.keys(WEB_BUNDLE_PATH_ROOTS).map((name) => [name, 1000]),
);

test("lazy feature catalog admits 96 distinct entries and rejects 97", () => {
  const features = Array.from({ length: 96 }, (_, index) => ({
    entry: `src/features/Deferred${index}.tsx`,
    maxGzipBytes: 1000,
    reason: "Explicitly opened deferred surface.",
  }));
  assert.deepEqual(parseLazyFeatureBudgets(features), features);
  assert.throws(
    () =>
      parseLazyFeatureBudgets([
        ...features,
        { ...features[0], entry: "src/features/Overflow.tsx" },
      ]),
    /lazy feature chunk budgets are missing or invalid/,
  );
});

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
          webPathJsGzipBytes: pathLimits,
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

async function terminalCandidate(terminalLimit = 100000) {
  const root = await candidate({ dynamic: "ordinary dynamic" });
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
  await writeFile(path.join(root, "dist/assets/terminal.css"), ".xterm {}");
  const manifestPath = path.join(root, "dist/.vite/manifest.json");
  const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
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
  await writeFile(manifestPath, JSON.stringify(manifest));
  const budgetPath = path.join(root, "docs/performance/budgets.json");
  const budgets = JSON.parse(await readFile(budgetPath, "utf8"));
  budgets.bundle.webTotalJsGzipBytes = 100000;
  budgets.bundle.webTerminalLazyJsGzipBytes = terminalLimit;
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
  return root;
}

// Emitted terminal chunks as the desktop build writes them: identical code
// that imports shared chunks by their content-hashed file names.
async function writeTerminalChunks(root, [shared, deps, port, dock], code) {
  const assets = path.join(root, "dist/assets");
  await writeFile(
    path.join(assets, "terminal.js"),
    `import{a as e}from"./${shared}.js";import{t}from"./${deps}.js";` +
      `import{p as n}from"./${port}.js";${code}`,
  );
  await writeFile(
    path.join(assets, "terminal-shell.js"),
    `import{a as e}from"./${shared}.js";import{p as n}from"./${port}.js";` +
      "const r=()=>import(`./" +
      dock +
      ".js`);export{r as TerminalDockShell};",
  );
  await writeFile(
    path.join(assets, "terminal-port.js"),
    `import{a as e}from"./${shared}.js";const p={open:e};export{p};`,
  );
}

const terminalCode =
  "var xterm=class{constructor(e){this.rows=e.rows;this.cols=e.cols}" +
  "write(e){return n.write(this.rows,this.cols,e)}};export{xterm as T};";

test("terminal budget ignores which content hashes the chunks import", async () => {
  const root = await terminalCandidate();
  try {
    await writeTerminalChunks(
      root,
      ["DLlys00o", "CcCufrF-2", "CHrBYBpZ2", "MjEmMWCT2"],
      terminalCode,
    );
    const first = verifyWebBundle(root);
    await writeTerminalChunks(
      root,
      ["BLP16Psh", "DqQcVKt_2", "CNAtOgS52", "DHLqKPLh2"],
      terminalCode,
    );
    const second = verifyWebBundle(root);
    assert.notEqual(
      first.terminalLazyEmittedGzipBytes,
      second.terminalLazyEmittedGzipBytes,
    );
    assert.equal(first.terminalLazyGzipBytes, second.terminalLazyGzipBytes);
    assert.equal(
      second.coreTotalGzipBytes,
      second.totalGzipBytes - second.terminalLazyEmittedGzipBytes,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("terminal budget still counts one byte of terminal code growth", async () => {
  const hashes = ["DLlys00o", "CcCufrF-2", "CHrBYBpZ2", "MjEmMWCT2"];
  const baselineRoot = await terminalCandidate();
  let limit;
  try {
    await writeTerminalChunks(baselineRoot, hashes, terminalCode);
    limit = verifyWebBundle(baselineRoot).terminalLazyGzipBytes;
  } finally {
    await rm(baselineRoot, { recursive: true, force: true });
  }
  const root = await terminalCandidate(limit);
  try {
    await writeTerminalChunks(root, hashes, terminalCode);
    assert.equal(verifyWebBundle(root).terminalLazyGzipBytes, limit);
    await writeTerminalChunks(
      root,
      hashes,
      terminalCode.replace("this.rows=e.rows", "this.rows=+e.rows"),
    );
    assert.throws(
      () => verifyWebBundle(root),
      (error) => {
        const match = /terminal lazy gzip (\d+) exceeds (\d+)/.exec(
          error.message,
        );
        assert.ok(match);
        assert.equal(Number(match[2]), limit);
        assert.ok(Number(match[1]) > limit);
        return true;
      },
    );
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
        webPathJsGzipBytes: pathLimits,
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
  for (const key of new Set(Object.values(WEB_BUNDLE_PATH_ROOTS).flat())) {
    if (key === "index.html") continue;
    manifest[key] = { file: "assets/dynamic.js", imports: ["index.html"] };
    manifest["_dynamic.js"].dynamicImports ??= [];
    manifest["_dynamic.js"].dynamicImports.push(key);
  }
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

test("a shrinking core total cannot hide an independently regressed user path", async () => {
  const root = await candidate();
  try {
    const manifestPath = path.join(root, "dist/.vite/manifest.json");
    const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
    manifest["src/features/diff/DiffSurface.tsx"].file = "assets/diff.js";
    manifest._unused = { file: "assets/unused.js" };
    manifest["index.html"].dynamicImports.push("_unused");
    await writeFile(manifestPath, JSON.stringify(manifest));
    await writeFile(path.join(root, "dist/assets/diff.js"), "b");
    await writeFile(
      path.join(root, "dist/assets/unused.js"),
      "0123456789abcdef".repeat(600),
    );
    const before = verifyWebBundle(root);
    const budgetPath = path.join(root, "docs/performance/budgets.json");
    const budgets = JSON.parse(await readFile(budgetPath, "utf8"));
    budgets.bundle.webPathJsGzipBytes.editorDiff =
      before.paths.editorDiff.gzipBytes;
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
    assert.doesNotThrow(() => verifyWebBundle(root));
    await writeFile(path.join(root, "dist/assets/unused.js"), "c");
    await writeFile(
      path.join(root, "dist/assets/diff.js"),
      "export const diff = true;",
    );
    const after = {
      paths: measureWebBundlePaths(root, manifest),
      coreTotalGzipBytes: (
        await Promise.all(
          ["entry.js", "imported.js", "dynamic.js", "diff.js", "unused.js"].map(
            (file) => readFile(path.join(root, "dist/assets", file)),
          ),
        )
      ).reduce((sum, contents) => sum + gzipSync(contents).length, 0),
    };
    assert.ok(after.coreTotalGzipBytes < before.coreTotalGzipBytes);
    assert.ok(
      after.paths.editorDiff.gzipBytes > before.paths.editorDiff.gzipBytes,
    );
    assert.throws(
      () => verifyWebBundle(root),
      /editorDiff static closure gzip.*exceeds/,
    );
    delete budgets.bundle.webPathJsGzipBytes;
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
    assert.throws(
      () => verifyWebBundle(root),
      /path limit.*missing or invalid/,
    );
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
          webPathJsGzipBytes: pathLimits,
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
          webPathJsGzipBytes: pathLimits,
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
          webPathJsGzipBytes: pathLimits,
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
          webPathJsGzipBytes: pathLimits,
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

const featureKey = "src/features/chat/Feature.tsx";

async function lazyFeatureCandidate() {
  const root = await candidate();
  const manifestPath = path.join(root, "dist/.vite/manifest.json");
  const budgetPath = path.join(root, "docs/performance/budgets.json");
  const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
  const budgets = JSON.parse(await readFile(budgetPath, "utf8"));
  manifest["index.html"].dynamicImports.push(featureKey);
  manifest[featureKey] = {
    file: "assets/feature.js",
    src: featureKey,
    isDynamicEntry: true,
    imports: ["index.html", "_feature-only.js", "_imported.js"],
  };
  manifest["_feature-only.js"] = { file: "assets/feature-only.js" };
  await writeFile(
    path.join(root, "dist/assets/feature.js"),
    "feature chunk ".repeat(40),
  );
  await writeFile(
    path.join(root, "dist/assets/feature-only.js"),
    "feature helper ".repeat(40),
  );
  budgets.bundle.lazyFeatureChunks = [
    { entry: featureKey, maxGzipBytes: 1000, reason: "Test feature." },
  ];
  const write = async () => {
    await writeFile(manifestPath, JSON.stringify(manifest));
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
  };
  await write();
  return { root, manifest, budgets, write };
}

test("a lazy feature budget counts only its exclusive files and leaves the core", async () => {
  const { root } = await lazyFeatureCandidate();
  try {
    const result = verifyWebBundle(root);
    const feature = result.lazyFeatureChunks[featureKey];
    // The shared static import stays in the core; the feature's own chunk and
    // its exclusive helper move out of it.
    assert.deepEqual(feature.files, [
      "assets/feature-only.js",
      "assets/feature.js",
    ]);
    const exclusive = await Promise.all(
      ["feature.js", "feature-only.js"].map((file) =>
        readFile(path.join(root, "dist/assets", file)),
      ),
    );
    assert.equal(
      feature.gzipBytes,
      exclusive.reduce((sum, contents) => sum + gzipSync(contents).length, 0),
    );
    assert.equal(
      result.coreTotalGzipBytes,
      result.totalGzipBytes - feature.gzipBytes,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("a lazy feature fails at one byte over its own cap", async () => {
  const { root, budgets, write } = await lazyFeatureCandidate();
  try {
    const measured =
      verifyWebBundle(root).lazyFeatureChunks[featureKey].gzipBytes;
    budgets.bundle.lazyFeatureChunks[0].maxGzipBytes = measured;
    await write();
    assert.doesNotThrow(() => verifyWebBundle(root));
    budgets.bundle.lazyFeatureChunks[0].maxGzipBytes = measured - 1;
    await write();
    assert.throws(
      () => verifyWebBundle(root),
      new RegExp(`${featureKey} lazy gzip ${measured} exceeds ${measured - 1}`),
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("a lazy feature budget ignores imported content hashes but counts code growth", async () => {
  const { root } = await lazyFeatureCandidate();
  const featureFile = path.join(root, "dist/assets/feature.js");
  const code =
    "const s=e.useMemo(()=>t.snapshot(n),[n]);export{s as ChangesSurface};";
  const chunk = (shared, deps, body = code) =>
    `import{a as e}from"./${shared}.js";import{t}from"./${deps}.js";${body}`;
  try {
    await writeFile(featureFile, chunk("DLlys00o", "CPZB2TWi"));
    const first = verifyWebBundle(root);
    const firstEmitted = gzipSync(await readFile(featureFile)).length;
    await writeFile(featureFile, chunk("snapshot", "useMemo_"));
    const second = verifyWebBundle(root);
    const measured = first.lazyFeatureChunks[featureKey].gzipBytes;
    // As emitted, the two chunks compress to different sizes.
    assert.notEqual(gzipSync(await readFile(featureFile)).length, firstEmitted);
    assert.equal(second.lazyFeatureChunks[featureKey].gzipBytes, measured);
    await writeFile(
      featureFile,
      chunk("snapshot", "useMemo_", code.replace("[n]", "[+n]")),
    );
    assert.ok(
      verifyWebBundle(root).lazyFeatureChunks[featureKey].gzipBytes > measured,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("only import specifiers lose their content hashes", () => {
  const masked = "./xxxxxxxx.js";
  for (const [source, expected] of [
    ['import{a as e}from"./DLlys00o.js";', `import{a as e}from"${masked}";`],
    ["import './DLlys00o.js';", `import '${masked}';`],
    ["await import(`./DLlys00o.js`)", `await import(\`${masked}\`)`],
    ['import ( "./DLlys00o.js" )', `import ( "${masked}" )`],
    ['import"./fObb2P8k2.js";', 'import"./xxxxxxxxx.js";'],
    ['export{a}from"./DLlys00o.js";', `export{a}from"${masked}";`],
    ['export*from"./DLlys00o.js";', `export*from"${masked}";`],
  ]) {
    assert.equal(normalizeHashedImports(source), expected);
  }
  for (const source of [
    'export const file="./aaaaaaaa.js";',
    "const file='./aaaaaaaa.js';",
    "const file=`./aaaaaaaa.js`;",
    'load("./aaaaaaaa.js");',
    'const reimport="./aaaaaaaa.js";',
    `const text='import("./DLlys00o.js")';`,
    `const text='import{a}from"./DLlys00o.js"';`,
    'const text=`import("./DLlys00o.js")`;',
    '/* import("./DLlys00o.js") */const a=1;',
    '// import"./DLlys00o.js"\nconst a=1;',
    'import(name+"./DLlys00o.js");',
  ]) {
    assert.equal(normalizeHashedImports(source), source);
  }
  // The preload helper's stylesheet list loses only its hashes; the same
  // names anywhere else are measured as written.
  const preload = (hash) =>
    `const __vite__mapDeps=(i,m=__vite__mapDeps,d=(m.f||(m.f=["assets/index-${hash}.css","assets/TerminalDock-B1njb2Cr.css"])))=>i.map(i=>d[i]);const css="assets/index-${hash}.css";`;
  assert.equal(
    normalizeHashedImports(preload("BYhftfGF")),
    preload("BYhftfGF").replace(
      '"assets/index-BYhftfGF.css","assets/TerminalDock-B1njb2Cr.css"',
      '"assets/index-xxxxxxxx.css","assets/TerminalDock-xxxxxxxx.css"',
    ),
  );
  assert.equal(
    normalizeHashedImports(preload("BYhftfGF")).replace(
      /BYhftfGF/g,
      "CWtMMHlB",
    ),
    normalizeHashedImports(preload("CWtMMHlB")),
  );
  const malformed = 'import("./DLlys00o.js");const x=;';
  assert.equal(normalizeHashedImports(malformed), malformed);
  // A longer chunk-like string is real growth and still measures larger.
  const grown = (name) =>
    gzipSync(normalizeHashedImports(`export const file="./${name}.js";`))
      .length;
  assert.ok(grown("aaaaaaaa1") > grown("aaaaaaaa"));
  const quoted = (name) =>
    gzipSync(normalizeHashedImports(`const text='import("./${name}.js")';`))
      .length;
  assert.notEqual(quoted("DLlys00o"), quoted("aaaaaaaa"));
});

test("a lazy feature must be a reachable dynamic entry", async () => {
  const { root, manifest, write } = await lazyFeatureCandidate();
  try {
    manifest[featureKey].isDynamicEntry = false;
    await write();
    assert.throws(() => verifyWebBundle(root), /reachable dynamic entry/);
    manifest[featureKey].isDynamicEntry = true;
    manifest["index.html"].dynamicImports = manifest[
      "index.html"
    ].dynamicImports.filter((key) => key !== featureKey);
    await write();
    assert.throws(() => verifyWebBundle(root), /reachable dynamic entry/);
    delete manifest[featureKey];
    await write();
    assert.throws(() => verifyWebBundle(root), /reachable dynamic entry/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("a lazy feature rejects initial and eager imports", async () => {
  const { root, manifest, write } = await lazyFeatureCandidate();
  try {
    manifest["index.html"].imports.push(featureKey);
    await write();
    assert.throws(() => verifyWebBundle(root), /initial static closure/);
    manifest["index.html"].imports.pop();
    manifest["_dynamic.js"].imports = [featureKey];
    await write();
    assert.throws(() => verifyWebBundle(root), /statically imported/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("a lazy feature rejects a chunk shared with another entry", async () => {
  const { root, manifest, write } = await lazyFeatureCandidate();
  try {
    manifest._other = { file: "assets/feature.js" };
    await write();
    assert.throws(() => verifyWebBundle(root), /shared with another/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("lazy feature budgets reject malformed, duplicate and reserved entries", async () => {
  const { root, budgets, write } = await lazyFeatureCandidate();
  const valid = budgets.bundle.lazyFeatureChunks[0];
  try {
    for (const lazyFeatureChunks of [
      {},
      [{ ...valid, reason: "" }],
      [{ ...valid, maxGzipBytes: 0 }],
      [{ ...valid, extra: true }],
      [{ entry: valid.entry, maxGzipBytes: 1000 }],
      [{ ...valid, entry: "src/../outside.ts" }],
      [valid, valid],
      [{ ...valid, entry: "src/workbench/SubagentSurface.tsx" }],
    ]) {
      budgets.bundle.lazyFeatureChunks = lazyFeatureChunks;
      await write();
      assert.throws(
        () => verifyWebBundle(root),
        /lazy feature chunk budgets are missing or invalid/,
      );
    }
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

async function lazyFeatureWithCss() {
  const candidateFiles = await lazyFeatureCandidate();
  const { root, manifest, budgets, write } = candidateFiles;
  manifest[featureKey].css = ["assets/feature.css"];
  await writeFile(
    path.join(root, "dist/assets/feature.css"),
    ".feature{color:red}".repeat(20),
  );
  budgets.bundle.lazyFeatureChunks[0].maxCssGzipBytes = 1000;
  await write();
  return candidateFiles;
}

test("a lazy feature budgets its own CSS against its own cap", async () => {
  const { root, budgets, write } = await lazyFeatureWithCss();
  try {
    const feature = verifyWebBundle(root).lazyFeatureChunks[featureKey];
    const css = await readFile(path.join(root, "dist/assets/feature.css"));
    assert.deepEqual(feature.cssFiles, ["assets/feature.css"]);
    assert.equal(feature.cssGzipBytes, gzipSync(css).length);
    budgets.bundle.lazyFeatureChunks[0].maxCssGzipBytes =
      feature.cssGzipBytes - 1;
    await write();
    assert.throws(
      () => verifyWebBundle(root),
      new RegExp(`${featureKey} lazy CSS gzip ${feature.cssGzipBytes} exceeds`),
    );
    delete budgets.bundle.lazyFeatureChunks[0].maxCssGzipBytes;
    await write();
    assert.throws(() => verifyWebBundle(root), /must budget its lazy CSS/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("lazy feature CSS never leaks into the initial CSS", async () => {
  const { root, manifest, write } = await lazyFeatureWithCss();
  try {
    manifest["index.html"].css = ["assets/feature.css"];
    await write();
    assert.throws(() => verifyWebBundle(root), /CSS entered the initial CSS/);
    delete manifest["index.html"].css;
    manifest["_dynamic.js"].css = ["assets/feature.css"];
    await write();
    assert.throws(() => verifyWebBundle(root), /CSS is shared/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
