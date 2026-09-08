import { gzipSync } from "node:zlib";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { webBuildSourceFingerprint } from "./web-build-fingerprint.mjs";

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

export function verifyWebBundle(root) {
  const budgets = json(
    path.join(root, "docs/performance/budgets.json"),
    "budgets",
  );
  const entryLimit = budgets.bundle?.webEntryJsGzipBytes;
  const initialStaticClosureLimit =
    budgets.bundle?.webInitialStaticClosureGzipBytes;
  const totalLimit = budgets.bundle?.webTotalJsGzipBytes;
  if (
    !Number.isInteger(entryLimit) ||
    !Number.isInteger(initialStaticClosureLimit) ||
    !Number.isInteger(totalLimit) ||
    entryLimit < 1 ||
    initialStaticClosureLimit < 1 ||
    totalLimit < 1
  )
    fail("web bundle limits are missing or invalid");
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
  const entries = Object.entries(manifest).filter(
    ([, record]) => record?.isEntry && record?.src === "index.html",
  );
  if (entries.length !== 1)
    fail("manifest must contain exactly one index.html entry");
  const entry = entries[0][1];
  if (!entry.file?.endsWith(".js"))
    fail("entry output is missing or not JavaScript");
  const initial = new Set();
  const visit = (record) => {
    if (!record || typeof record !== "object")
      fail("manifest import record is malformed");
    assetPath(root, record.file);
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
  for (const file of jsFiles) {
    if (
      readFileSync(path.join(assets, file), "utf8").includes(
        BROWSER_FIXTURE_SENTINEL,
      )
    ) {
      fail("desktop artifact contains a browser fixture sentinel");
    }
  }
  const exceeded = [];
  if (entryGzipBytes > entryLimit)
    exceeded.push(`entry gzip ${entryGzipBytes} exceeds ${entryLimit}`);
  if (initialStaticClosureGzipBytes > initialStaticClosureLimit) {
    exceeded.push(
      `initial static closure gzip ${initialStaticClosureGzipBytes} exceeds ${initialStaticClosureLimit}`,
    );
  }
  if (totalGzipBytes > totalLimit)
    exceeded.push(`total gzip ${totalGzipBytes} exceeds ${totalLimit}`);
  if (exceeded.length) fail(exceeded.join("; "));
  return {
    entryGzipBytes,
    initialStaticClosureGzipBytes,
    totalGzipBytes,
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
