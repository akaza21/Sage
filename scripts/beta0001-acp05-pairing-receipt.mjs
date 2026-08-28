import { createHash } from "node:crypto";
import { lstat, open } from "node:fs/promises";
import path from "node:path";

const receiptMaximumBytes = 8_192;
const artifactMaximumBytes = 4 * 1024 * 1024;

export const ACP05_PAIRING_ARTIFACT_PATHS = Object.freeze([
  ".playwright-cli/page-2026-08-28T17-54-28-284Z.png",
  ".playwright-cli/page-2026-08-28T17-54-41-816Z.yml",
  ".playwright-cli/page-2026-08-28T17-54-46-862Z.png",
]);

function reject(code) {
  throw new Error(`acp05-pairing-${code}`);
}

function validHash(value) {
  return typeof value === "string" && /^[0-9a-f]{64}$/u.test(value);
}

function exactKeys(value, keys) {
  return (
    value &&
    !Array.isArray(value) &&
    typeof value === "object" &&
    JSON.stringify(Object.keys(value).sort()) ===
      JSON.stringify([...keys].sort())
  );
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function plainFile(stat) {
  return stat.isFile() && !stat.isSymbolicLink() && stat.nlink === 1n;
}

async function readArtifact(artifactRoot, relativePath) {
  const file = path.join(artifactRoot, relativePath);
  const before = await lstat(file, { bigint: true });
  if (!plainFile(before) || before.size > BigInt(artifactMaximumBytes))
    reject("artifact-invalid");
  const handle = await open(file, "r");
  try {
    const opened = await handle.stat({ bigint: true });
    if (
      !plainFile(opened) ||
      opened.dev !== before.dev ||
      opened.ino !== before.ino ||
      opened.size !== before.size ||
      opened.mtimeNs !== before.mtimeNs
    )
      reject("artifact-invalid");
    const bytes = Buffer.allocUnsafe(Number(opened.size));
    await handle.read(bytes, 0, bytes.length, 0);
    const after = await handle.stat({ bigint: true });
    if (
      after.dev !== opened.dev ||
      after.ino !== opened.ino ||
      after.size !== opened.size ||
      after.mtimeNs !== opened.mtimeNs
    )
      reject("artifact-invalid");
    return bytes;
  } finally {
    await handle.close();
  }
}

export async function validateAcp05PairingReceipt({
  receiptBytes,
  artifactRoot,
  expectedEvidenceDocumentSha256,
}) {
  if (
    !Buffer.isBuffer(receiptBytes) ||
    receiptBytes.length > receiptMaximumBytes ||
    !validHash(expectedEvidenceDocumentSha256)
  )
    reject("schema-invalid");
  let receipt;
  try {
    receipt = JSON.parse(receiptBytes.toString("utf8"));
  } catch {
    reject("malformed-json");
  }
  if (
    !exactKeys(receipt, [
      "schemaVersion",
      "workItem",
      "status",
      "campaignBinding",
      "evidenceDocumentSha256",
      "artifacts",
    ]) ||
    receipt.schemaVersion !== 1 ||
    receipt.workItem !== "BETA-0001" ||
    receipt.status !== "observed" ||
    receipt.campaignBinding !== "package-receipt-required" ||
    receipt.evidenceDocumentSha256 !== expectedEvidenceDocumentSha256 ||
    !Array.isArray(receipt.artifacts) ||
    receipt.artifacts.length !== ACP05_PAIRING_ARTIFACT_PATHS.length
  )
    reject("schema-invalid");
  for (const [index, artifact] of receipt.artifacts.entries()) {
    if (
      !exactKeys(artifact, ["relativePath", "sha256"]) ||
      artifact.relativePath !== ACP05_PAIRING_ARTIFACT_PATHS[index] ||
      !validHash(artifact.sha256)
    )
      reject("schema-invalid");
    const bytes = await readArtifact(artifactRoot, artifact.relativePath);
    if (sha256(bytes) !== artifact.sha256) reject("artifact-substituted");
  }
  return {
    status: receipt.status,
    campaignBinding: receipt.campaignBinding,
  };
}
