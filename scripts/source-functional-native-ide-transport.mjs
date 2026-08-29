import { createHash } from "node:crypto";
import {
  closeSync,
  fstatSync,
  lstatSync,
  openSync,
  opendirSync,
  readSync,
  realpathSync,
} from "node:fs";
import path from "node:path";
import { runBoundedTool } from "./source-functional-native-ide-bounded-tool.mjs";

export {
  createPlaywrightTransportReceipt,
  validatePlaywrightTransportReceipt,
} from "./source-functional-native-ide-transport-receipt.mjs";

const MAX_NODE_BYTES = 256 * 1024 * 1024;
const MAX_SHIM_BYTES = 16 * 1024;
const MAX_PACKAGE_FILE_BYTES = 8 * 1024 * 1024;
const MAX_PACKAGE_TREE_BYTES = 64 * 1024 * 1024;
const MAX_PACKAGE_FILES = 2048;
const TREE_LIMITS = Object.freeze({
  maxDepth: 32,
  maxEntries: 4096,
  maxRelativePathBytes: 1024,
  maxTotalRelativePathBytes: 1024 * 1024,
});

function fail(code) {
  throw new Error(`journey-${code}`);
}

function exactKeys(value, keys) {
  return (
    value &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    Object.keys(value).sort().join("\0") === [...keys].sort().join("\0")
  );
}

function isAbsolutePath(value) {
  return (
    typeof value === "string" &&
    (path.isAbsolute(value) || path.win32.isAbsolute(value))
  );
}

function samePath(left, right) {
  const a = path.resolve(left);
  const b = path.resolve(right);
  return process.platform === "win32"
    ? a.toLowerCase() === b.toLowerCase()
    : a === b;
}

function assertCanonicalAbsolute(value) {
  if (
    !isAbsolutePath(value) ||
    path.normalize(value) !== value ||
    !samePath(value, path.resolve(value))
  )
    fail("playwright-transport-invalid");
  return path.resolve(value);
}

function boundedTreeLimits(requested = {}) {
  if (
    !requested ||
    typeof requested !== "object" ||
    Array.isArray(requested) ||
    Object.keys(requested).some((name) => !(name in TREE_LIMITS)) ||
    Object.values(requested).some(
      (value) => !Number.isSafeInteger(value) || value <= 0,
    )
  )
    fail("playwright-transport-invalid");
  return Object.fromEntries(
    Object.entries(TREE_LIMITS).map(([name, maximum]) => [
      name,
      Math.min(requested[name] ?? maximum, maximum),
    ]),
  );
}

function assertPlainAncestors(target, finalKind) {
  const resolved = assertCanonicalAbsolute(target);
  const root = path.parse(resolved).root;
  let current = root;
  const segments = path
    .relative(root, resolved)
    .split(path.sep)
    .filter(Boolean);
  for (const [index, segment] of segments.entries()) {
    current = path.join(current, segment);
    const stats = lstatSync(current);
    if (
      stats.isSymbolicLink() ||
      !samePath(realpathSync.native(current), current)
    )
      fail("playwright-transport-invalid");
    const isFinal = index === segments.length - 1;
    if ((!isFinal || finalKind === "directory") && !stats.isDirectory())
      fail("playwright-transport-invalid");
    if (isFinal && finalKind === "file" && !stats.isFile())
      fail("playwright-transport-invalid");
  }
  return resolved;
}

function captureFile(
  filePath,
  maximumBytes,
  context,
  deps = {},
  retain = true,
) {
  const resolved = assertPlainAncestors(filePath, "file");
  const before = lstatSync(resolved, { bigint: true });
  if (before.size <= 0n || before.size > BigInt(maximumBytes))
    fail("playwright-transport-invalid");
  const descriptor = openSync(resolved, "r");
  const opened = fstatSync(descriptor, { bigint: true });
  const digest = createHash("sha256");
  const chunks = [];
  const chunk = Buffer.allocUnsafe(1024 * 1024);
  try {
    for (const key of ["dev", "ino", "size", "mtimeNs"])
      if (opened[key] !== before[key]) fail("playwright-transport-mutated");
    let count;
    while ((count = readSync(descriptor, chunk, 0, chunk.length, null)) > 0) {
      const bytes = Buffer.from(chunk.subarray(0, count));
      digest.update(bytes);
      if (retain) chunks.push(bytes);
      deps.afterRead?.({ ...context, path: resolved, bytesRead: count });
    }
    const openedAfter = fstatSync(descriptor, { bigint: true });
    const pathAfter = lstatSync(resolved, { bigint: true });
    for (const key of ["dev", "ino", "size", "mtimeNs"])
      if (openedAfter[key] !== opened[key] || pathAfter[key] !== opened[key])
        fail("playwright-transport-mutated");
  } finally {
    closeSync(descriptor);
  }
  return {
    identity: Object.freeze({
      path: resolved,
      bytes: Number(opened.size),
      sha256: digest.digest("hex"),
    }),
    content: retain ? Buffer.concat(chunks) : null,
    stats: opened,
  };
}

function listPlainTree(root, limits, openDirectory = opendirSync) {
  const canonicalRoot = assertPlainAncestors(root, "directory");
  const entries = [];
  const files = [];
  let totalRelativePathBytes = 0;
  const visit = (directory, relativeDirectory = "", depth = 1) => {
    const handle = openDirectory(directory);
    try {
      while (true) {
        const entry = handle.readSync();
        if (entry === null) break;
        if (
          entries.length >= limits.maxEntries ||
          typeof entry.name !== "string" ||
          entry.name.length === 0 ||
          entry.name === "." ||
          entry.name === ".." ||
          entry.name.includes("/") ||
          entry.name.includes("\\")
        )
          fail("playwright-transport-invalid");
        const absolute = path.join(directory, entry.name);
        const relative = path
          .join(relativeDirectory, entry.name)
          .split(path.sep)
          .join("/");
        const relativePathBytes = Buffer.byteLength(relative, "utf8");
        totalRelativePathBytes += relativePathBytes;
        if (
          depth > limits.maxDepth ||
          relativePathBytes > limits.maxRelativePathBytes ||
          totalRelativePathBytes > limits.maxTotalRelativePathBytes
        )
          fail("playwright-transport-invalid");
        const stats = lstatSync(absolute);
        if (
          stats.isSymbolicLink() ||
          !samePath(realpathSync.native(absolute), absolute)
        )
          fail("playwright-transport-invalid");
        if (stats.isDirectory()) {
          entries.push({ absolute, relative, kind: "directory" });
          visit(absolute, relative, depth + 1);
        } else if (stats.isFile()) {
          const file = { absolute, relative, kind: "file" };
          entries.push(file);
          files.push(file);
        } else fail("playwright-transport-invalid");
        if (files.length > MAX_PACKAGE_FILES)
          fail("playwright-transport-invalid");
      }
    } finally {
      handle.closeSync();
    }
  };
  visit(canonicalRoot);
  const compare = (a, b) =>
    Buffer.from(a.relative).compare(Buffer.from(b.relative));
  return { entries: entries.sort(compare), files: files.sort(compare) };
}

function validateFileSnapshot(filePath, stats) {
  const current = lstatSync(filePath, { bigint: true });
  for (const key of ["dev", "ino", "size", "mtimeNs"])
    if (current[key] !== stats[key]) fail("playwright-transport-mutated");
}

function validateTreeSnapshot({
  root,
  entries,
  captured,
  limits,
  openDirectory,
}) {
  const current = listPlainTree(root, limits, openDirectory).entries.map(
    ({ kind, relative }) => `${kind}\0${relative}`,
  );
  const expected = entries.map(({ kind, relative }) => `${kind}\0${relative}`);
  if (current.join("\0") !== expected.join("\0"))
    fail("playwright-transport-mutated");
  for (const { file, stats } of captured)
    validateFileSnapshot(file.absolute, stats);
}

function capturePackageTree(root, expectedName, deps = {}) {
  const limits = boundedTreeLimits(deps.limits);
  const openDirectory = deps.openDirectory ?? opendirSync;
  const { entries, files } = listPlainTree(root, limits, openDirectory);
  if (files.length === 0) fail("playwright-transport-invalid");
  const treeDigest = createHash("sha256");
  const captured = [];
  let bytes = 0;
  let manifestBytes;
  for (const file of entries) {
    treeDigest.update(file.relative, "utf8");
    treeDigest.update("\0");
    treeDigest.update(file.kind === "directory" ? "D\0" : "F\0");
    if (file.kind === "directory") continue;
    const role =
      expectedName === "@playwright/cli" &&
      file.relative === "playwright-cli.js"
        ? "entry"
        : file.relative === "package.json"
          ? "manifest"
          : "package-file";
    const result = captureFile(
      file.absolute,
      MAX_PACKAGE_FILE_BYTES,
      { packageName: expectedName, relativePath: file.relative, role },
      deps,
    );
    bytes += result.identity.bytes;
    if (bytes > MAX_PACKAGE_TREE_BYTES) fail("playwright-transport-invalid");
    treeDigest.update(String(result.identity.bytes), "ascii");
    treeDigest.update("\0");
    treeDigest.update(result.content);
    treeDigest.update("\0");
    if (file.relative === "package.json") manifestBytes = result.content;
    captured.push({ file, stats: result.stats });
  }
  const snapshot = { root, entries, captured, limits, openDirectory };
  validateTreeSnapshot(snapshot);
  if (!manifestBytes) fail("playwright-transport-invalid");
  let manifest;
  try {
    manifest = JSON.parse(manifestBytes.toString("utf8"));
  } catch {
    fail("playwright-transport-invalid");
  }
  if (
    manifest?.name !== expectedName ||
    typeof manifest.version !== "string" ||
    manifest.version.length < 1 ||
    manifest.version.length > 128
  )
    fail("playwright-transport-invalid");
  return {
    identity: Object.freeze({
      root: path.resolve(root),
      name: expectedName,
      version: manifest.version,
      fileCount: files.length,
      bytes,
      treeSha256: treeDigest.digest("hex"),
    }),
    manifest,
    files: new Set(files.map(({ relative }) => relative)),
    snapshot,
  };
}

function expectedNpmPlaywrightShim() {
  return '@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\r\nIF EXIST "%dp0%\\node.exe" (\r\n  SET "_prog=%dp0%\\node.exe"\r\n) ELSE (\r\n  SET "_prog=node"\r\n  SET PATHEXT=%PATHEXT:;.JS;=;%\r\n)\r\n\r\nendLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & "%_prog%"  "%dp0%\\..\\@playwright\\cli\\playwright-cli.js" %*\r\n';
}

function dependencyMap(manifest, field) {
  const value = manifest[field] ?? {};
  if (!value || typeof value !== "object" || Array.isArray(value))
    fail("playwright-transport-invalid");
  for (const dependency of Object.values(value))
    if (typeof dependency !== "string" || dependency.length === 0)
      fail("playwright-transport-invalid");
  return value;
}

function validateRuntimeGraph(cli, playwright, core) {
  const cliDependencies = dependencyMap(cli.manifest, "dependencies");
  if (
    !Object.keys(cliDependencies).every((name) =>
      ["playwright", "playwright-core"].includes(name),
    ) ||
    cliDependencies.playwright !== playwright.identity.version ||
    (cliDependencies["playwright-core"] !== undefined &&
      cliDependencies["playwright-core"] !== core.identity.version)
  )
    fail("playwright-transport-invalid");
  const playwrightDependencies = dependencyMap(
    playwright.manifest,
    "dependencies",
  );
  if (
    !exactKeys(playwrightDependencies, ["playwright-core"]) ||
    playwrightDependencies["playwright-core"] !== core.identity.version
  )
    fail("playwright-transport-invalid");
  if (Object.keys(dependencyMap(core.manifest, "dependencies")).length !== 0)
    fail("playwright-transport-invalid");
  for (const current of [cli, playwright, core]) {
    if (Object.keys(dependencyMap(current.manifest, "peerDependencies")).length)
      fail("playwright-transport-invalid");
    for (const field of ["bundleDependencies", "bundledDependencies"])
      if (
        current.manifest[field] !== undefined &&
        (!Array.isArray(current.manifest[field]) ||
          current.manifest[field].length !== 0)
      )
        fail("playwright-transport-invalid");
    const optional = Object.keys(
      dependencyMap(current.manifest, "optionalDependencies"),
    );
    const allowed = current === playwright ? ["fsevents"] : [];
    if (optional.some((name) => !allowed.includes(name)))
      fail("playwright-transport-invalid");
  }
}

function resolveGraph(playwrightCli, deps = {}) {
  if (
    !isAbsolutePath(playwrightCli) ||
    path.basename(playwrightCli) !== "playwright-cli.cmd" ||
    path.basename(path.dirname(playwrightCli)) !== ".bin"
  )
    fail("playwright-transport-invalid");
  const shim = captureFile(
    playwrightCli,
    MAX_SHIM_BYTES,
    { role: "shim" },
    deps,
  );
  if (shim.content.toString("utf8") !== expectedNpmPlaywrightShim())
    fail("playwright-transport-invalid");
  const nodeModules = path.dirname(path.dirname(shim.identity.path));
  const cli = capturePackageTree(
    path.join(nodeModules, "@playwright", "cli"),
    "@playwright/cli",
    deps,
  );
  const playwright = capturePackageTree(
    path.join(nodeModules, "playwright"),
    "playwright",
    deps,
  );
  const playwrightCore = capturePackageTree(
    path.join(nodeModules, "playwright-core"),
    "playwright-core",
    deps,
  );
  if (
    !exactKeys(cli.manifest.bin, ["playwright-cli"]) ||
    cli.manifest.bin["playwright-cli"] !== "playwright-cli.js" ||
    !cli.files.has("playwright-cli.js")
  )
    fail("playwright-transport-invalid");
  validateRuntimeGraph(cli, playwright, playwrightCore);
  const entryPath = path.join(cli.identity.root, "playwright-cli.js");
  const node = captureFile(
    process.execPath,
    MAX_NODE_BYTES,
    { role: "node" },
    deps,
    false,
  );
  for (const current of [cli, playwright, playwrightCore])
    validateTreeSnapshot(current.snapshot);
  validateFileSnapshot(shim.identity.path, shim.stats);
  return Object.freeze({
    node: node.identity,
    shim: shim.identity,
    entryPath,
    packages: Object.freeze({
      cli: cli.identity,
      playwright: playwright.identity,
      playwrightCore: playwrightCore.identity,
    }),
  });
}

export function resolvePlaywrightTransport(playwrightCli, deps = {}) {
  try {
    return resolveGraph(playwrightCli, deps);
  } catch (error) {
    if (
      /^journey-playwright-transport-(?:invalid|mutated)$/u.test(error?.message)
    )
      throw error;
    fail("playwright-transport-invalid");
  }
}

export function validatePlaywrightTransport(transport, deps = {}) {
  if (!exactKeys(transport, ["entryPath", "node", "packages", "shim"]))
    fail("playwright-transport-invalid");
  const current = resolvePlaywrightTransport(transport.shim?.path, deps);
  if (JSON.stringify(current) !== JSON.stringify(transport))
    fail("playwright-transport-mutated");
  return transport;
}

export function runPlaywrightTool(transport, args, deadline, cwd, deps = {}) {
  if (!Array.isArray(args) || args.some((value) => typeof value !== "string"))
    fail("playwright-arguments-invalid");
  const {
    runTool = runBoundedTool,
    validateTransport = validatePlaywrightTransport,
    ...toolDeps
  } = deps;
  validateTransport(transport);
  return runTool(
    transport.node.path,
    [transport.entryPath, ...args],
    deadline,
    cwd,
    { ...toolDeps, expectedExecutablePath: transport.node.path },
  );
}
