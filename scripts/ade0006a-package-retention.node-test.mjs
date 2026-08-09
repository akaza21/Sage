import assert from "node:assert/strict";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import {
  rename as renameAsync,
  rm as rmAsync,
  writeFile as writeFileAsync,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { withAdeCampaignAdmission } from "./ade0006a-admission.mjs";
import {
  failPackageCampaign,
  finalizePackageCampaign,
  reservePackageCampaign,
} from "./ade0006a-package-retention.mjs";
import { cleanupPackageCandidate } from "./ade0006a-package-cleanup.mjs";
import {
  withAdmissionLock,
  writeLedger,
} from "./ade0006a-package-retention-ledger.mjs";

const digest = (character) => character.repeat(64);

function frozen(overrides = {}) {
  return {
    contractManifestSha256: digest("a"),
    packageContractSha256: digest("b"),
    package: {
      observerBounds: {
        memberManifestShardsPerInstaller: 8,
        memberManifestBytesPerInstaller: 6 * 1024 * 1024,
      },
      retention: {
        rawReservation: {
          sourceManifestFiles: 1,
          sourceManifestBytes: 8 * 1024 * 1024,
          sourceManifestEntries: 4096,
          installerMemberSets: 4,
          memberSidecarFiles: 32,
          memberSidecarBytes: 24 * 1024 * 1024,
          totalFiles: 33,
          totalBytes: 32 * 1024 * 1024,
        },
        collectionFailureCodes: [
          "build-failed",
          "inspection-failed",
          "source-drift",
          "receipt-validation-failed",
          "collection-failed",
        ],
      },
    },
    evidence: {
      adeCampaignAdmission: {
        root: "local-evidence/ade0006a",
        admissionLock: "local-evidence/ade0006a/.admission-lock",
        serializedPhases: ["Package", "C0-B"],
        rule: "one ADE-wide admission lock",
      },
      campaignBounds: {
        rawFiles: 64,
        rawBytes: 64 * 1024 * 1024,
        campaignsPerCandidateIdentity: 8,
        rawBytesPerCandidateIdentity: 512 * 1024 * 1024,
        retainedCandidateIdentities: 2,
        globalFiles: 1024,
        globalBytes: 1024 * 1024 * 1024,
        curatedBytes: 16 * 1024 * 1024,
        ...overrides,
      },
      packageCampaignRetention: {
        admissionLock: "local-evidence/ade0006a/.admission-lock",
        ledgerBytes: 64 * 1024,
        ledgerEntries: 16,
        cleanupRecords: 16,
        reservation: {
          rawFiles: 33,
          rawBytes: 32 * 1024 * 1024,
          sourceManifestFiles: 1,
          sourceManifestBytes: 8 * 1024 * 1024,
          sourceManifestEntries: 4096,
          memberSidecarFiles: 32,
          memberSidecarBytes: 24 * 1024 * 1024,
          curatedFiles: 1,
          curatedBytes: 16 * 1024 * 1024,
        },
      },
    },
  };
}

function reservation(
  root,
  campaignId = "package-pair-01",
  value = frozen(),
  allocationIdentity = digest("c"),
) {
  return reservePackageCampaign({
    sourceRoot: root,
    campaignId,
    allocationIdentity,
    sourceAggregateSha256: digest("d"),
    startedAtUtc: "2026-08-06T00:00:00.000Z",
    frozen: value,
  });
}

async function reserveAndFail(root, campaignId, identity, value) {
  await reservation(root, campaignId, value, identity);
  await failPackageCampaign({
    sourceRoot: root,
    campaignId,
    allocationIdentity: identity,
    completedAtUtc: "2026-08-06T00:01:00.000Z",
    failureCode: "collection-failed",
    frozen: value,
  });
}

function ledger(root) {
  return JSON.parse(
    readFileSync(
      path.join(
        root,
        "local-evidence",
        "ade0006a-package",
        "retention-ledger-v1.json",
      ),
      "utf8",
    ),
  );
}

test("Package admission holds the one ADE-wide campaign lock", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-retention-"));
  const value = frozen();
  const lockPath = path.join(
    root,
    "local-evidence",
    "ade0006a",
    ".admission-lock",
  );
  try {
    await withAdmissionLock(root, value, async (packageRoot) => {
      assert.equal(
        packageRoot,
        path.join(root, "local-evidence", "ade0006a-package"),
      );
      assert.equal(existsSync(lockPath), true);
      await assert.rejects(
        withAdeCampaignAdmission({
          sourceRoot: root,
          frozen: value,
          phase: "C0-B",
          action: async () => undefined,
        }),
        /capacity-rejected/,
      );
    });
    assert.equal(existsSync(lockPath), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("ADE admission rejects policy drift before creating a lock", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-retention-"));
  const value = frozen();
  value.evidence.packageCampaignRetention.admissionLock =
    "local-evidence/ade0006a-package/.admission-lock";
  try {
    await assert.rejects(
      withAdeCampaignAdmission({
        sourceRoot: root,
        frozen: value,
        phase: "Package",
        action: async () => undefined,
      }),
      /admission-policy-invalid/,
    );
    assert.equal(
      existsSync(path.join(root, "local-evidence", "ade0006a")),
      false,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("retention reserves before collection and records typed failure without raw diagnostics", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-retention-"));
  const value = frozen();
  try {
    const reserved = await reservation(root, "package-pair-01", value);
    assert.equal(ledger(root).entries[0].state, "active");
    assert.equal(reserved.entry.reservedRawFiles, 33);
    await assert.rejects(
      reservation(root, "package-pair-01", value),
      /campaign-id-conflict/,
    );
    await failPackageCampaign({
      sourceRoot: root,
      campaignId: "package-pair-01",
      allocationIdentity: digest("c"),
      completedAtUtc: "2026-08-06T00:01:00.000Z",
      failureCode: "build-failed",
      frozen: value,
    });
    const failed = ledger(root).entries[0];
    assert.equal(failed.state, "failed-unsummarized");
    assert.equal(failed.failureCode, "build-failed");
    assert.equal(JSON.stringify(failed).includes("stderr"), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("retention rejects an over-limit reservation before creating a campaign", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-retention-"));
  try {
    await assert.rejects(
      reservation(root, "package-pair-02", frozen({ rawFiles: 32 })),
      /capacity-rejected/,
    );
    assert.equal(
      existsSync(
        path.join(
          root,
          "local-evidence",
          "ade0006a-package",
          "retention-ledger-v1.json",
        ),
      ),
      false,
    );
    assert.equal(
      existsSync(
        path.join(
          root,
          "local-evidence",
          "ade0006a-package",
          "package-pair-02",
        ),
      ),
      false,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("retention preserves bounded headroom for referenced, diagnostic, and next identities", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-retention-"));
  const value = frozen({ retainedCandidateIdentities: 3 });
  try {
    await reserveAndFail(root, "package-pair-11", digest("b"), value);
    await reserveAndFail(root, "package-pair-12", digest("c"), value);
    await reserveAndFail(root, "package-pair-13", digest("d"), value);
    await assert.rejects(
      reservation(root, "package-pair-14", value, digest("e")),
      /capacity-rejected/,
    );
    assert.equal(ledger(root).entries.length, 3);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("explicit cleanup removes one obsolete identity and records every removed path", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-retention-"));
  const value = frozen({ retainedCandidateIdentities: 3 });
  try {
    await reserveAndFail(root, "package-pair-21", digest("b"), value);
    await reserveAndFail(root, "package-pair-22", digest("b"), value);
    await reserveAndFail(root, "package-pair-23", digest("c"), value);
    const cleanup = await cleanupPackageCandidate({
      sourceRoot: root,
      cleanupId: "package-cleanup-01",
      allocationIdentity: digest("b"),
      completedAtUtc: "2026-08-06T00:02:00.000Z",
      frozen: value,
    });
    assert.deepEqual(
      cleanup.removedCampaigns.map(({ campaignId }) => campaignId),
      ["package-pair-21", "package-pair-22"],
    );
    assert.equal(ledger(root).entries.length, 1);
    assert.equal(ledger(root).cleanups.length, 1);
    for (const campaignId of ["package-pair-21", "package-pair-22"]) {
      assert.equal(
        existsSync(
          path.join(root, "local-evidence", "ade0006a-package", campaignId),
        ),
        false,
      );
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("explicit cleanup never deletes the sole unsummarized failure", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-retention-"));
  const value = frozen();
  try {
    await reserveAndFail(root, "package-pair-24", digest("b"), value);
    await assert.rejects(
      cleanupPackageCandidate({
        sourceRoot: root,
        cleanupId: "package-cleanup-02",
        allocationIdentity: digest("b"),
        completedAtUtc: "2026-08-06T00:02:00.000Z",
        frozen: value,
      }),
      /sole-unsummarized-failure/,
    );
    assert.equal(ledger(root).entries.length, 1);
    assert.equal(ledger(root).cleanups.length, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("retention atomically publishes the exact receipt with final ledger state", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-retention-"));
  const value = frozen();
  try {
    const reserved = await reservation(root, "package-pair-03", value);
    writeFileSync(path.join(reserved.campaignRoot, "raw.json"), "raw");
    const receiptBytes = Buffer.from("receipt");
    const curated = path.join(
      root,
      "docs",
      "work-items",
      "ADE-0006A-acp-session-foundation",
      "evidence",
      "package",
      "package-pair-03.json",
    );
    const receipt = {
      campaignId: "package-pair-03",
      allocationIdentity: digest("c"),
      result: "failed",
      retention: {
        rawFiles: 1,
        rawBytes: 3,
        curatedFiles: 1,
        curatedBytes: receiptBytes.byteLength,
      },
    };
    await finalizePackageCampaign({
      sourceRoot: root,
      campaignId: receipt.campaignId,
      allocationIdentity: receipt.allocationIdentity,
      completedAtUtc: "2026-08-06T00:01:00.000Z",
      receipt,
      receiptBytes,
      frozen: value,
    });
    assert.equal(ledger(root).entries[0].state, "completed-failed");
    assert.deepEqual(readFileSync(curated), receiptBytes);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("retention rolls back receipt publication when final ledger write fails", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-retention-"));
  const value = frozen();
  try {
    const reserved = await reservation(root, "package-pair-04", value);
    writeFileSync(path.join(reserved.campaignRoot, "raw.json"), "raw");
    const receiptBytes = Buffer.from("receipt");
    const receipt = {
      campaignId: "package-pair-04",
      allocationIdentity: digest("c"),
      result: "passed",
      retention: {
        rawFiles: 1,
        rawBytes: 3,
        curatedFiles: 1,
        curatedBytes: receiptBytes.byteLength,
      },
    };
    const curated = path.join(
      root,
      "docs",
      "work-items",
      "ADE-0006A-acp-session-foundation",
      "evidence",
      "package",
      "package-pair-04.json",
    );
    value.evidence.packageCampaignRetention.ledgerBytes = Buffer.byteLength(
      JSON.stringify(ledger(root)),
    );
    await assert.rejects(
      finalizePackageCampaign({
        sourceRoot: root,
        campaignId: receipt.campaignId,
        allocationIdentity: receipt.allocationIdentity,
        completedAtUtc: "2026-08-06T00:01:00.000Z",
        receipt,
        receiptBytes,
        frozen: value,
      }),
      /capacity-rejected/,
    );
    assert.equal(existsSync(curated), false);
    assert.equal(ledger(root).entries[0].state, "active");
    value.evidence.packageCampaignRetention.ledgerBytes = 64 * 1024;
    await failPackageCampaign({
      sourceRoot: root,
      campaignId: receipt.campaignId,
      allocationIdentity: receipt.allocationIdentity,
      completedAtUtc: "2026-08-06T00:02:00.000Z",
      failureCode: "collection-failed",
      frozen: value,
    });
    assert.equal(ledger(root).entries[0].state, "failed-unsummarized");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("retention removes an incomplete next ledger after a partial write failure", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-retention-"));
  const packageRoot = path.join(root, "local-evidence", "ade0006a-package");
  const nextPath = path.join(packageRoot, "retention-ledger-v1.next.json");
  mkdirSync(packageRoot, { recursive: true });
  try {
    const error = new Error("no space left on device");
    error.code = "ENOSPC";
    await assert.rejects(
      writeLedger(
        packageRoot,
        {
          schemaVersion: 1,
          workItem: "ADE-0006A",
          phase: "Package",
          entries: [],
          cleanups: [],
        },
        frozen(),
        {
          writeFile: async (target) => {
            await writeFileAsync(target, "", { flag: "wx" });
            throw error;
          },
          rename: renameAsync,
          rm: rmAsync,
        },
      ),
      (value) => value === error,
    );
    assert.equal(existsSync(nextPath), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
