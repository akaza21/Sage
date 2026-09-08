import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { webBuildSourceFingerprint } from "./web-build-fingerprint.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const target = process.argv[2];
if (target !== "desktop-native") {
  throw new Error("web-build-receipt: expected desktop-native build target");
}
let emittedTarget;
try {
  emittedTarget = JSON.parse(
    readFileSync(
      path.join(root, "dist", ".sage-web-build-target.json"),
      "utf8",
    ),
  );
} catch {
  throw new Error(
    "web-build-receipt: desktop build target evidence is missing or malformed",
  );
}
if (emittedTarget?.schemaVersion !== 1 || emittedTarget.target !== target) {
  throw new Error(
    "web-build-receipt: desktop build target evidence is invalid",
  );
}
const fingerprint = webBuildSourceFingerprint(root);
const receiptPath = path.join(root, "dist", ".sage-web-build-receipt.json");
mkdirSync(path.dirname(receiptPath), { recursive: true });
writeFileSync(
  receiptPath,
  `${JSON.stringify({ schemaVersion: 2, target, gzip: "node:zlib.gzipSync defaults", source: fingerprint }, null, 2)}\n`,
);
