import path from "node:path";

const SHA256 = /^[a-f0-9]{64}$/u;
const MAX_PACKAGE_TREE_BYTES = 64 * 1024 * 1024;
const MAX_PACKAGE_FILES = 2048;
const PACKAGE_KEYS = ["cli", "playwright", "playwrightCore"];

function fail() {
  throw new Error("journey-playwright-transport-invalid");
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

function validFileIdentity(value) {
  return (
    exactKeys(value, ["bytes", "path", "sha256"]) &&
    isAbsolutePath(value.path) &&
    Number.isSafeInteger(value.bytes) &&
    value.bytes > 0 &&
    SHA256.test(value.sha256)
  );
}

function validPackageIdentity(value, expectedName) {
  return (
    exactKeys(value, [
      "bytes",
      "fileCount",
      "name",
      "root",
      "treeSha256",
      "version",
    ]) &&
    value.name === expectedName &&
    isAbsolutePath(value.root) &&
    typeof value.version === "string" &&
    value.version.length > 0 &&
    value.version.length <= 128 &&
    Number.isSafeInteger(value.fileCount) &&
    value.fileCount > 0 &&
    value.fileCount <= MAX_PACKAGE_FILES &&
    Number.isSafeInteger(value.bytes) &&
    value.bytes > 0 &&
    value.bytes <= MAX_PACKAGE_TREE_BYTES &&
    SHA256.test(value.treeSha256)
  );
}

export function validatePlaywrightTransportReceipt(value) {
  if (
    !exactKeys(value, ["node", "packages", "shim"]) ||
    !validFileIdentity(value.node) ||
    !validFileIdentity(value.shim) ||
    !exactKeys(value.packages, PACKAGE_KEYS) ||
    !validPackageIdentity(value.packages.cli, "@playwright/cli") ||
    !validPackageIdentity(value.packages.playwright, "playwright") ||
    !validPackageIdentity(value.packages.playwrightCore, "playwright-core")
  )
    fail();
  return Object.freeze({
    node: Object.freeze({ ...value.node }),
    shim: Object.freeze({ ...value.shim }),
    packages: Object.freeze(
      Object.fromEntries(
        PACKAGE_KEYS.map((key) => [
          key,
          Object.freeze({ ...value.packages[key] }),
        ]),
      ),
    ),
  });
}

export function createPlaywrightTransportReceipt(transport) {
  return validatePlaywrightTransportReceipt({
    node: transport?.node,
    shim: transport?.shim,
    packages: transport?.packages,
  });
}
