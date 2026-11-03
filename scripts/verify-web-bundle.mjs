import { gzipSync } from "node:zlib";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { webBuildSourceFingerprint } from "./web-build-fingerprint.mjs";
import {
  measureWebBundlePaths,
  validateWebBundleManifest,
  verifyWebBundlePathBudgets,
} from "./web-bundle-paths.mjs";
import {
  measureLazyFeatures,
  parseLazyFeatureBudgets,
} from "./web-bundle-lazy-features.mjs";

const DESKTOP_NATIVE_TARGET = "desktop-native";
const BROWSER_FIXTURE_SENTINEL =
  "Simulated browser fixture — not live repository state.";

function fail(message) {
  throw new Error(`web-bundle: ${message}`);
}

function json(file, label) {
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch {
    fail(`${label} is missing or malformed`);
  }
}

function emittedBuildTarget(root) {
  const target = json(
    path.join(root, "dist", ".sage-web-build-target.json"),
    "desktop build target evidence",
  );
  if (target.schemaVersion !== 1 || target.target !== DESKTOP_NATIVE_TARGET) {
    fail("desktop build target evidence is invalid");
  }
  return target.target;
}

function assetPath(root, file) {
  if (
    typeof file !== "string" ||
    !file.startsWith("assets/") ||
    file.includes("..") ||
    path.isAbsolute(file)
  ) {
    fail("manifest contains an escaping asset path");
  }
  const result = path.resolve(root, "dist", file);
  if (
    !result.startsWith(`${path.resolve(root, "dist", "assets")}${path.sep}`) ||
    !existsSync(result)
  )
    fail(`asset is missing: ${file}`);
  return result;
}

function gzipBytes(file) {
  const contents = readFileSync(file);
  if (contents.byteLength === 0) fail(`asset is empty: ${file}`);
  return gzipSync(contents).byteLength;
}

// The desktop build emits every chunk as `assets/[hash].js`: an 8-character
// base64url content hash, plus a numeric suffix when names collide. Chunks
// import siblings by `./<name>.js`, so a change to any shared chunk rewrites
// these names without touching the importing chunk's code. Each one becomes a
// fixed token of the same length before a per-chunk lazy budget gzips it.
// The core total still subtracts the chunks' emitted sizes.
// Entry, initial-closure and core-total measurements stay as emitted.
const HASHED_RELATIVE_IMPORT =
  /(?<=["'`])\.\/([A-Za-z0-9_-]{8}\d*)\.js(?=["'`])/gu;

export function normalizeHashedImports(source) {
  return source.replace(
    HASHED_RELATIVE_IMPORT,
    (_, name) => `./${"x".repeat(name.length)}.js`,
  );
}

function lazyChunkGzipBytes(file) {
  if (!file.endsWith(".js")) return gzipBytes(file);
  const contents = readFileSync(file, "utf8");
  if (contents.length === 0) fail(`asset is empty: ${file}`);
  return gzipSync(normalizeHashedImports(contents)).byteLength;
}

export function verifyWebBundle(root) {
  const budgets = json(
    path.join(root, "docs/performance/budgets.json"),
    "budgets",
  );
  const entryLimit = budgets.bundle?.webEntryJsGzipBytes;
  const initialStaticClosureLimit =
    budgets.bundle?.webInitialStaticClosureGzipBytes;
  const totalLimit = budgets.bundle?.webTotalJsGzipBytes;
  const terminalLimit = budgets.bundle?.webTerminalLazyJsGzipBytes;
  const subagentLimit = budgets.bundle?.webSubagentLazyJsGzipBytes;
  if (
    !Number.isInteger(entryLimit) ||
    !Number.isInteger(initialStaticClosureLimit) ||
    !Number.isInteger(totalLimit) ||
    entryLimit < 1 ||
    initialStaticClosureLimit < 1 ||
    totalLimit < 1 ||
    (terminalLimit !== undefined &&
      (!Number.isInteger(terminalLimit) || terminalLimit < 1)) ||
    (subagentLimit !== undefined &&
      (!Number.isInteger(subagentLimit) || subagentLimit < 1))
  )
    fail("web bundle limits are missing or invalid");
  const lazyFeatures = parseLazyFeatureBudgets(
    budgets.bundle?.lazyFeatureChunks,
    [
      "src/workbench/TerminalDock.tsx",
      "src/workbench/TerminalDockShell.tsx",
      "src/workbench/SubagentSurface.tsx",
    ],
  );
  const receipt = json(
    path.join(root, "dist/.sage-web-build-receipt.json"),
    "build receipt",
  );
  const emittedTarget = emittedBuildTarget(root);
  const source = webBuildSourceFingerprint(root);
  if (
    receipt.schemaVersion !== 2 ||
    receipt.target !== DESKTOP_NATIVE_TARGET ||
    receipt.target !== emittedTarget ||
    receipt.gzip !== "node:zlib.gzipSync defaults" ||
    receipt.source?.sha256 !== source.sha256 ||
    receipt.source?.files !== source.files
  )
    fail("build receipt is stale or invalid");
  const manifest = json(
    path.join(root, "dist/.vite/manifest.json"),
    "Vite manifest",
  );
  validateWebBundleManifest(manifest);
  const entries = Object.entries(manifest).filter(
    ([, record]) => record?.isEntry && record?.src === "index.html",
  );
  if (entries.length !== 1)
    fail("manifest must contain exactly one index.html entry");
  const entry = entries[0][1];
  if (!entry.file?.endsWith(".js"))
    fail("entry output is missing or not JavaScript");
  const initial = new Set();
  const initialCss = new Set();
  const visit = (record) => {
    if (!record || typeof record !== "object")
      fail("manifest import record is malformed");
    assetPath(root, record.file);
    for (const css of record.css ?? []) initialCss.add(assetPath(root, css));
    if (!record.file.endsWith(".js") || initial.has(record.file)) return;
    initial.add(record.file);
    for (const imported of record.imports ?? []) {
      if (typeof imported !== "string" || !manifest[imported])
        fail("manifest import is missing");
      visit(manifest[imported]);
    }
  };
  visit(entry);
  const entryGzipBytes = gzipBytes(assetPath(root, entry.file));
  const initialStaticClosureGzipBytes = [...initial].reduce(
    (sum, file) => sum + gzipBytes(assetPath(root, file)),
    0,
  );
  const assets = path.join(root, "dist/assets");
  if (!existsSync(assets)) fail("assets directory is missing");
  const jsFiles = readdirSync(assets).filter((file) => file.endsWith(".js"));
  if (jsFiles.length === 0) fail("no JavaScript assets were emitted");
  const totalGzipBytes = jsFiles.reduce(
    (sum, file) => sum + gzipBytes(path.join(assets, file)),
    0,
  );
  let terminalLazyGzipBytes = 0;
  let terminalLazyEmittedGzipBytes = 0;
  let subagentLazyGzipBytes = 0;
  let coreTotalGzipBytes = totalGzipBytes;
  const reachable = new Set();
  const visitReachable = (key) => {
    if (reachable.has(key)) return;
    const record = manifest[key];
    if (!record) fail("manifest reachable import is missing");
    reachable.add(key);
    for (const imported of [
      ...(record.imports ?? []),
      ...(record.dynamicImports ?? []),
    ])
      visitReachable(imported);
  };
  visitReachable(entries[0][0]);
  if (terminalLimit !== undefined) {
    const terminalKey = "src/workbench/TerminalDock.tsx";
    const terminal = manifest[terminalKey];
    if (
      !terminal?.isDynamicEntry ||
      !terminal.file?.endsWith(".js") ||
      !reachable.has(terminalKey)
    ) {
      fail("TerminalDock must be a reachable lazy JavaScript chunk");
    }
    if (
      Object.entries(manifest).some(
        ([key, record]) =>
          key !== terminalKey && record?.file === terminal.file,
      )
    ) {
      fail(
        "TerminalDock lazy emitted file is shared by another manifest entry",
      );
    }
    const terminalFile = assetPath(root, terminal.file);
    if (initial.has(terminal.file))
      fail("xterm TerminalDock entered initial static closure");
    if (
      !Array.isArray(terminal.css) ||
      terminal.css.length === 0 ||
      terminal.css.some((css) => {
        const resolved = assetPath(root, css);
        return (
          initialCss.has(resolved) ||
          Object.entries(manifest).some(
            ([key, record]) =>
              key !== terminalKey && record?.css?.includes(css),
          )
        );
      })
    ) {
      fail("xterm CSS must belong only to the lazy TerminalDock branch");
    }
    const packageJson = json(
      path.join(root, "package.json"),
      "package manifest",
    );
    const xtermPackages = Object.keys(packageJson.dependencies ?? {}).filter(
      (name) => name.startsWith("@xterm/"),
    );
    if (JSON.stringify(xtermPackages) !== JSON.stringify(["@xterm/xterm"])) {
      fail("only @xterm/xterm core is admitted; no xterm addons");
    }
    const sourceRoot = path.join(root, "src");
    const sourceImports = [];
    const scan = (directory) => {
      for (const entry of readdirSync(directory, { withFileTypes: true })) {
        const full = path.join(directory, entry.name);
        if (entry.isDirectory()) scan(full);
        else if (
          entry.isFile() &&
          /\.[cm]?[jt]sx?$/u.test(entry.name) &&
          !/\.test\.[cm]?[jt]sx?$/u.test(entry.name) &&
          readFileSync(full, "utf8").includes("@xterm/")
        ) {
          sourceImports.push(
            path.relative(root, full).replaceAll(path.sep, "/"),
          );
        }
      }
    };
    scan(sourceRoot);
    if (
      JSON.stringify(sourceImports) !== JSON.stringify([terminalKey]) ||
      !readFileSync(path.join(root, terminalKey), "utf8").includes(
        '"@xterm/xterm"',
      )
    ) {
      fail("@xterm must be imported only by TerminalDock");
    }
    if (readdirSync(assets).some((file) => file.endsWith(".map"))) {
      fail("TerminalDock production build must not emit source maps");
    }
    if (
      !readFileSync(terminalFile, "utf8").includes("xterm") ||
      jsFiles.some(
        (file) =>
          file !== path.basename(terminal.file) &&
          readFileSync(path.join(assets, file), "utf8").includes("xterm"),
      )
    ) {
      fail("xterm appears outside the TerminalDock lazy chunk");
    }
    const shellKey = "src/workbench/TerminalDockShell.tsx";
    const shell = manifest[shellKey];
    if (
      !shell?.isDynamicEntry ||
      !shell.file?.endsWith(".js") ||
      !reachable.has(shellKey) ||
      !(shell.dynamicImports ?? []).includes(terminalKey) ||
      shell.file === terminal.file ||
      Object.entries(manifest).some(
        ([key, record]) => key !== shellKey && record?.file === shell.file,
      )
    ) {
      fail(
        "TerminalDockShell must be a separate reachable lazy terminal chrome chunk",
      );
    }
    const sharedKeys = (shell.imports ?? []).filter(
      (key) =>
        (terminal.imports ?? []).includes(key) &&
        manifest[key]?.name === "userTerminalDesktop",
    );
    if (sharedKeys.length !== 1)
      fail("Terminal lazy port chunk is missing or ambiguous");
    const portKey = sharedKeys[0];
    const port = manifest[portKey];
    if (!port?.file?.endsWith(".js") || !reachable.has(portKey))
      fail("Terminal lazy port chunk is missing or ambiguous");
    const terminalFiles = [terminal.file, shell.file, port.file];
    if (terminalFiles.some((file) => initial.has(file)))
      fail("Terminal lazy chunk entered initial static closure");
    if (
      Object.entries(manifest).some(
        ([key, record]) =>
          key !== shellKey &&
          key !== terminalKey &&
          key !== portKey &&
          [
            ...(record.imports ?? []),
            ...(record.dynamicImports ?? []),
          ].includes(portKey),
      )
    ) {
      fail("Terminal lazy port must be imported only by terminal chunks");
    }
    terminalLazyGzipBytes = terminalFiles.reduce(
      (sum, file) => sum + lazyChunkGzipBytes(assetPath(root, file)),
      0,
    );
    terminalLazyEmittedGzipBytes = terminalFiles.reduce(
      (sum, file) => sum + gzipBytes(assetPath(root, file)),
      0,
    );
    coreTotalGzipBytes -= terminalLazyEmittedGzipBytes;
  }
  if (subagentLimit !== undefined) {
    const subagentKey = "src/workbench/SubagentSurface.tsx";
    const subagent = manifest[subagentKey];
    if (
      !subagent?.isDynamicEntry ||
      !subagent.file?.endsWith(".js") ||
      !reachable.has(subagentKey) ||
      initial.has(subagent.file) ||
      Object.entries(manifest).some(
        ([key, record]) =>
          key !== subagentKey && record?.file === subagent.file,
      )
    ) {
      fail("SubagentSurface must own a reachable, noninitial lazy chunk");
    }
    subagentLazyGzipBytes = lazyChunkGzipBytes(assetPath(root, subagent.file));
    coreTotalGzipBytes -= gzipBytes(assetPath(root, subagent.file));
  }
  for (const file of jsFiles) {
    if (
      readFileSync(path.join(assets, file), "utf8").includes(
        BROWSER_FIXTURE_SENTINEL,
      )
    ) {
      fail("desktop artifact contains a browser fixture sentinel");
    }
  }
  const lazyFeatureChunks = measureLazyFeatures({
    manifest,
    entryKey: entries[0][0],
    reachable,
    initial,
    isInitialCss: (css) => initialCss.has(assetPath(root, css)),
    features: lazyFeatures,
    gzipFile: (file) => lazyChunkGzipBytes(assetPath(root, file)),
  });
  for (const feature of Object.values(lazyFeatureChunks))
    for (const file of feature.files)
      coreTotalGzipBytes -= gzipBytes(assetPath(root, file));
  const exceeded = [];
  if (entryGzipBytes > entryLimit)
    exceeded.push(`entry gzip ${entryGzipBytes} exceeds ${entryLimit}`);
  if (initialStaticClosureGzipBytes > initialStaticClosureLimit) {
    exceeded.push(
      `initial static closure gzip ${initialStaticClosureGzipBytes} exceeds ${initialStaticClosureLimit}`,
    );
  }
  if (coreTotalGzipBytes > totalLimit)
    exceeded.push(
      `core total gzip excluding budgeted lazy chunks ${coreTotalGzipBytes} exceeds ${totalLimit}`,
    );
  if (terminalLimit !== undefined && terminalLazyGzipBytes > terminalLimit)
    exceeded.push(
      `terminal lazy gzip ${terminalLazyGzipBytes} exceeds ${terminalLimit}`,
    );
  if (subagentLimit !== undefined && subagentLazyGzipBytes > subagentLimit)
    exceeded.push(
      `subagent lazy gzip ${subagentLazyGzipBytes} exceeds ${subagentLimit}`,
    );
  if (exceeded.length) fail(exceeded.join("; "));
  const paths = measureWebBundlePaths(root, manifest);
  verifyWebBundlePathBudgets(paths, budgets.bundle?.webPathJsGzipBytes);
  return {
    paths,
    entryGzipBytes,
    initialStaticClosureGzipBytes,
    totalGzipBytes,
    terminalLazyGzipBytes,
    terminalLazyEmittedGzipBytes,
    subagentLazyGzipBytes,
    lazyFeatureChunks,
    coreTotalGzipBytes,
    initialFiles: [...initial].sort(),
    jsFiles: jsFiles.sort(),
  };
}

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    console.log(JSON.stringify(verifyWebBundle(root), null, 2));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
