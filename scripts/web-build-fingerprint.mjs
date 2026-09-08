import { createHash } from "node:crypto";
import { lstatSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";

export const WEB_BUILD_FINGERPRINT_LIMITS = Object.freeze({
  maxFiles: 10_000,
  maxDepth: 32,
  maxBytes: 32 * 1024 * 1024,
});

const defaultSourceRoots = [
  "src",
  "scripts",
  "docs/performance/budgets.json",
  "index.html",
  "package.json",
  "package-lock.json",
  "vite.config.ts",
  "tsconfig.json",
];

function fail(message) {
  throw new Error(`web-build-fingerprint: ${message}`);
}

function validLimits(limits) {
  for (const key of ["maxFiles", "maxDepth", "maxBytes"]) {
    if (
      !Number.isInteger(limits[key]) ||
      limits[key] < (key === "maxDepth" ? 0 : 1)
    ) {
      fail(`invalid ${key} limit`);
    }
  }
}

function collect(root, relative, state, limits, depth) {
  if (depth > limits.maxDepth) fail(`maximum depth exceeded at ${relative}`);
  const absolute = path.join(root, relative);
  const entries = readdirSync(absolute, { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name),
  );
  for (const entry of entries) {
    const child = path.join(relative, entry.name);
    const childAbsolute = path.join(root, child);
    const stat = lstatSync(childAbsolute);
    if (stat.isDirectory()) {
      collect(root, child, state, limits, depth + 1);
      continue;
    }
    if (!stat.isFile()) fail(`nonregular source path: ${child}`);
    addFile(child, stat.size, state, limits);
  }
}

function addFile(relative, bytes, state, limits) {
  if (state.files.length >= limits.maxFiles) {
    fail(`maximum file count exceeded at ${relative}`);
  }
  if (state.bytes + bytes > limits.maxBytes) {
    fail(`maximum byte count exceeded at ${relative}`);
  }
  state.files.push(relative);
  state.bytes += bytes;
}

export function webBuildSourceFingerprint(root, options = {}) {
  const limits = options.limits ?? WEB_BUILD_FINGERPRINT_LIMITS;
  const sourceRoots = options.sourceRoots ?? defaultSourceRoots;
  validLimits(limits);
  if (!Array.isArray(sourceRoots) || sourceRoots.length === 0) {
    fail("source roots are missing or invalid");
  }
  const state = { files: [], bytes: 0 };
  for (const source of sourceRoots) {
    if (typeof source !== "string" || source.length === 0) {
      fail("source root is invalid");
    }
    const absolute = path.join(root, source);
    let stat;
    try {
      stat = lstatSync(absolute);
    } catch {
      fail(`source path is missing: ${source}`);
    }
    if (stat.isDirectory()) {
      collect(root, source, state, limits, 0);
    } else if (stat.isFile()) {
      addFile(source, stat.size, state, limits);
    } else {
      fail(`nonregular source path: ${source}`);
    }
  }
  const digest = createHash("sha256");
  for (const file of state.files.sort()) {
    digest.update(file.replaceAll(path.sep, "/"));
    digest.update("\0");
    digest.update(readFileSync(path.join(root, file)));
    digest.update("\0");
  }
  return { files: state.files.length, sha256: digest.digest("hex") };
}
