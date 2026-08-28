import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  ACP05_PAIRING_ARTIFACT_PATHS,
  validateAcp05PairingReceipt,
} from "./beta0001-acp05-pairing-receipt.mjs";

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

async function fixture() {
  const root = await mkdtemp(path.join(os.tmpdir(), "sage-acp05-pairing-"));
  const artifacts = [];
  for (const relativePath of ACP05_PAIRING_ARTIFACT_PATHS) {
    const file = path.join(root, relativePath);
    await mkdir(path.dirname(file), { recursive: true });
    await writeFile(file, relativePath);
    artifacts.push({ relativePath, sha256: sha256(Buffer.from(relativePath)) });
  }
  const evidenceDocument = Buffer.from("native pairing evidence");
  return {
    root,
    receipt: {
      schemaVersion: 1,
      workItem: "BETA-0001",
      status: "observed",
      campaignBinding: "package-receipt-required",
      evidenceDocumentSha256: sha256(evidenceDocument),
      artifacts,
    },
    evidenceDocument,
  };
}

test("accepts the bounded observed pairing receipt without candidate admission", async () => {
  const value = await fixture();
  try {
    assert.deepEqual(
      await validateAcp05PairingReceipt({
        receiptBytes: Buffer.from(JSON.stringify(value.receipt)),
        artifactRoot: value.root,
        expectedEvidenceDocumentSha256: sha256(value.evidenceDocument),
      }),
      {
        status: "observed",
        campaignBinding: "package-receipt-required",
      },
    );
  } finally {
    await rm(value.root, { recursive: true, force: true });
  }
});

test("rejects substituted pairing artifacts", async () => {
  const value = await fixture();
  try {
    await writeFile(
      path.join(value.root, ACP05_PAIRING_ARTIFACT_PATHS[0]),
      "substituted",
    );
    await assert.rejects(
      validateAcp05PairingReceipt({
        receiptBytes: Buffer.from(JSON.stringify(value.receipt)),
        artifactRoot: value.root,
        expectedEvidenceDocumentSha256: sha256(value.evidenceDocument),
      }),
      /acp05-pairing-artifact-substituted/u,
    );
  } finally {
    await rm(value.root, { recursive: true, force: true });
  }
});

test("rejects an attempted package-free campaign binding", async () => {
  const value = await fixture();
  try {
    value.receipt.campaignBinding = "bound";
    await assert.rejects(
      validateAcp05PairingReceipt({
        receiptBytes: Buffer.from(JSON.stringify(value.receipt)),
        artifactRoot: value.root,
        expectedEvidenceDocumentSha256: sha256(value.evidenceDocument),
      }),
      /acp05-pairing-schema-invalid/u,
    );
  } finally {
    await rm(value.root, { recursive: true, force: true });
  }
});
