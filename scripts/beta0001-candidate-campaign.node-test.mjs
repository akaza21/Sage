import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  link,
  mkdtemp,
  readFile,
  rm,
  symlink,
  unlink,
  writeFile,
} from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

const repositoryRoot = process.cwd();
const thresholdPath = path.join(
  repositoryRoot,
  "docs/work-items/BETA-0001-primary-beta-path/contracts/beta-acceptance-v1.json",
);
const sourceCommit = "a".repeat(40);
const signerThumbprint = "b".repeat(40);
const installerSha256 = "c".repeat(64);
const installedExecutableSha256 = "d".repeat(64);
const ids = [
  "windows.source-package",
  "windows.installed-lifecycle",
  "projects.workbench-multiroot",
  "resources.explorer",
  "editor.file-diff-recovery-find",
  "search.workspace",
  "git.basic",
  "chat.lifecycle-handoff",
  "acp.durable-edit-summary",
  "codex.account-models",
  "codex.multichat-turn-restart-cancel",
  "codex.process-convergence",
  "renderer.a11y-reload-focus",
  "security.authority-secrets",
  "recovery.crash-cleanup",
  "performance.resource-storage",
  "macos.shared-contract-ci",
  "review.standards",
  "review.spec",
  "test.independent",
];
const quartet = [
  "renderer.a11y-reload-focus",
  "security.authority-secrets",
  "recovery.crash-cleanup",
  "performance.resource-storage",
];

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

function fixtureAuthenticodeCatalog() {
  const admittedThumbprints = new Set([signerThumbprint]);
  return {
    verify: (thumbprint) => admittedThumbprints.has(thumbprint),
  };
}

async function campaignAdmission() {
  return import("./beta0001-candidate-campaign.mjs");
}

async function thresholdAuthority() {
  const bytes = await readFile(thresholdPath);
  return { value: JSON.parse(bytes), sha256: sha256(bytes) };
}

function candidate(commit = sourceCommit) {
  return { platform: "windows", architecture: "x86_64", sourceCommit: commit };
}

function metricDefinitions(threshold) {
  const journey = threshold.codexJourney;
  const budgets = threshold.codexBudgets;
  const minimum = (name, value) => [name, value];
  const maximum = (name, value) => [name, value];
  return new Map([
    [
      "codex.account-models",
      [minimum("observedModels", journey.minimumObservedModels)],
    ],
    [
      "codex.multichat-turn-restart-cancel",
      [
        minimum("distinctChats", journey.minimumDistinctChats),
        minimum(
          "sequentialTurnsInOneSession",
          journey.minimumSequentialTurnsInOneSession,
        ),
        minimum("successfulTurns", journey.minimumSuccessfulTurns),
        minimum(
          "restartResumeThenTurnCycles",
          journey.minimumRestartResumeThenTurnCycles,
        ),
        minimum("cancelThenReuseCycles", journey.minimumCancelThenReuseCycles),
        minimum("cleanCycles", journey.minimumCleanCycles),
        minimum("failureCycles", journey.minimumFailureCycles),
      ],
    ],
    [
      "codex.process-convergence",
      [
        maximum(
          "orphanProcessesAfterClose",
          journey.maximumOrphanProcessesAfterClose,
        ),
      ],
    ],
    [
      "security.authority-secrets",
      [
        maximum(
          "secretExposureObservations",
          journey.maximumSecretExposureObservations,
        ),
        maximum(
          "unauthorizedMutationObservations",
          journey.maximumUnauthorizedMutationObservations,
        ),
        maximum(
          "unexpectedListenerCount",
          budgets.unexpectedListenerCountMaximum,
        ),
        maximum(
          "unexpectedEnvironmentEntryCount",
          budgets.unexpectedEnvironmentEntryCountMaximum,
        ),
      ],
    ],
    [
      "performance.resource-storage",
      Object.entries(budgets)
        .filter(
          ([name]) =>
            ![
              "unexpectedListenerCountMaximum",
              "unexpectedEnvironmentEntryCountMaximum",
            ].includes(name),
        )
        .map(([name, value]) =>
          name === "minimumLatencySamples"
            ? minimum("latencySamples", value)
            : maximum(name.replace(/Maximum$/u, ""), value),
        ),
    ],
  ]);
}

async function fixture({ statuses = {}, omit = [], aggregate = {} } = {}) {
  const root = await mkdtemp(
    path.join(os.tmpdir(), "beta0001-candidate-campaign-"),
  );
  const { value: threshold, sha256: contractSha256 } =
    await thresholdAuthority();
  const definitions = metricDefinitions(threshold);
  const evidence = ids.map((id) => ({
    id,
    status: statuses[id] ?? (omit.includes(id) ? "missing" : "passed"),
  }));
  for (const { id, status } of evidence) {
    if (omit.includes(id)) continue;
    const metrics = Object.fromEntries(definitions.get(id) ?? []);
    await writeFile(
      path.join(root, `${id}.json`),
      JSON.stringify({
        schemaVersion: 1,
        evidenceId: id,
        contractSha256,
        candidate: candidate(),
        status: statuses[id] ?? "passed",
        metrics,
      }),
    );
    void status;
  }
  const result = {
    schemaVersion: 1,
    workItem: "BETA-0001",
    contractSha256,
    candidate: candidate(),
    evidence,
    verdict: "passed",
    ...aggregate,
  };
  return {
    root,
    contractSha256,
    resultBytes: Buffer.from(JSON.stringify(result)),
    authenticodeCatalog: fixtureAuthenticodeCatalog(),
  };
}

async function writePackageSidecars(
  value,
  { receipt = {}, admission = {}, authenticode = true } = {},
) {
  const receiptValue = {
    schemaVersion: 1,
    workItem: "BETA-0001",
    sourceCommit,
    installerSha256,
    installedExecutableSha256,
    signerThumbprint,
    authenticode,
    ...receipt,
  };
  const receiptBytes = Buffer.from(JSON.stringify(receiptValue));
  await writeFile(path.join(value.root, "package-receipt.json"), receiptBytes);
  const admissionValue = {
    schemaVersion: 1,
    workItem: "BETA-0001",
    sourceCommit,
    contractSha256: value.contractSha256,
    packageReceiptSha256: sha256(receiptBytes),
    installerSha256,
    installedExecutableSha256,
    ...admission,
  };
  await writeFile(
    path.join(value.root, "package-admission.json"),
    JSON.stringify(admissionValue),
  );
  value.packageReceiptSha256 = sha256(receiptBytes);
  return { receiptBytes, packageReceiptSha256: value.packageReceiptSha256 };
}

async function journeyEvidenceFor(root) {
  const journeyEvidence = [];
  for (const id of quartet) {
    journeyEvidence.push({
      id,
      sha256: sha256(await readFile(path.join(root, `${id}.json`))),
    });
  }
  return journeyEvidence;
}

async function writeCampaignSidecars(
  value,
  {
    receipt = {},
    admission = {},
    receiptPadding = 0,
    admissionPadding = 0,
    journeyEvidence,
  } = {},
) {
  const rendererPairingReceiptBytes = Buffer.from(
    JSON.stringify({
      schemaVersion: 1,
      workItem: "BETA-0001",
      status: "observed",
      campaignBinding: "package-receipt-required",
      evidenceDocumentSha256: "e".repeat(64),
      artifacts: [],
    }),
  );
  await writeFile(
    path.join(value.root, "acp05-renderer-pairing-receipt.json"),
    rendererPairingReceiptBytes,
  );
  const receiptValue = {
    schemaVersion: 1,
    workItem: "BETA-0001",
    sourceCommit,
    packageReceiptSha256: value.packageReceiptSha256,
    installerSha256,
    campaignKind: "security-a11y-performance-recovery",
    rendererPairingReceiptSha256: sha256(rendererPairingReceiptBytes),
    journeyEvidence: journeyEvidence ?? (await journeyEvidenceFor(value.root)),
    ...receipt,
  };
  const receiptBytes = Buffer.concat([
    Buffer.from(JSON.stringify(receiptValue)),
    Buffer.alloc(receiptPadding, 0x20),
  ]);
  await writeFile(path.join(value.root, "campaign-receipt.json"), receiptBytes);
  const admissionValue = {
    schemaVersion: 1,
    workItem: "BETA-0001",
    sourceCommit,
    contractSha256: value.contractSha256,
    packageReceiptSha256: value.packageReceiptSha256,
    campaignReceiptSha256: sha256(receiptBytes),
    installerSha256,
    ...admission,
  };
  const admissionBytes = Buffer.concat([
    Buffer.from(JSON.stringify(admissionValue)),
    Buffer.alloc(admissionPadding, 0x20),
  ]);
  await writeFile(
    path.join(value.root, "campaign-admission.json"),
    admissionBytes,
  );
  return { receiptBytes, admissionBytes, rendererPairingReceiptBytes };
}

async function bound(value, options = {}) {
  const { receiptBytes } = await writePackageSidecars(value, options);
  const campaign = await writeCampaignSidecars(value, options);
  return {
    packageReceiptSha256: sha256(receiptBytes),
    campaignReceiptSha256: sha256(campaign.receiptBytes),
    receiptBytes: campaign.receiptBytes,
    admissionBytes: campaign.admissionBytes,
    rendererPairingReceiptBytes: campaign.rendererPairingReceiptBytes,
  };
}

async function validate(value, options = {}) {
  const { validateBetaCandidateCampaignAdmission } = await campaignAdmission();
  return validateBetaCandidateCampaignAdmission({
    resultBytes: value.resultBytes,
    evidenceRoot: value.root,
    expectedSourceCommit: sourceCommit,
    expectedPackageReceiptSha256:
      options.expectedPackageReceiptSha256 ?? value.packageReceiptSha256,
    expectedInstallerSha256: options.expectedInstallerSha256 ?? installerSha256,
    authenticodeCatalog:
      options.authenticodeCatalog ?? value.authenticodeCatalog,
    expectedCampaignReceiptSha256: options.expectedCampaignReceiptSha256,
    onAfterCampaignOpen: options.onAfterCampaignOpen,
    onAfterJourneyOpen: options.onAfterJourneyOpen,
    onAfterFirstJourneyBind: options.onAfterFirstJourneyBind,
  });
}

async function rejectsCode(action, code) {
  await assert.rejects(
    Promise.resolve().then(action),
    (error) => error?.code === code,
  );
}

async function close(value) {
  await rm(value.root, { recursive: true, force: true });
}

test("signed campaign admission returns the matrix verdict", async () => {
  const value = await fixture();
  try {
    const boundValue = await bound(value);
    assert.equal(
      (
        await validate(value, {
          expectedCampaignReceiptSha256: boundValue.campaignReceiptSha256,
        })
      ).verdict,
      "passed",
    );
  } finally {
    await close(value);
  }
});

test("missing authenticode fixture catalog is package-unsigned", async () => {
  const value = await fixture();
  try {
    const boundValue = await bound(value);
    value.authenticodeCatalog = undefined;
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: boundValue.campaignReceiptSha256,
        }),
      "package-unsigned",
    );
  } finally {
    await close(value);
  }
});

test("package-only fixture without campaign sidecars is campaign-missing", async () => {
  const value = await fixture();
  try {
    const { packageReceiptSha256 } = await writePackageSidecars(value);
    await rejectsCode(
      () =>
        validate(value, {
          expectedPackageReceiptSha256: packageReceiptSha256,
          expectedCampaignReceiptSha256: "e".repeat(64),
        }),
      "campaign-missing",
    );
  } finally {
    await close(value);
  }
});

test("missing campaign-receipt sidecar is campaign-missing", async () => {
  const value = await fixture();
  try {
    const boundValue = await bound(value);
    await unlink(path.join(value.root, "campaign-receipt.json"));
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: boundValue.campaignReceiptSha256,
        }),
      "campaign-missing",
    );
  } finally {
    await close(value);
  }
});

test("missing campaign-admission sidecar is campaign-missing", async () => {
  const value = await fixture();
  try {
    const boundValue = await bound(value);
    await unlink(path.join(value.root, "campaign-admission.json"));
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: boundValue.campaignReceiptSha256,
        }),
      "campaign-missing",
    );
  } finally {
    await close(value);
  }
});

test("package-missing still wins when only campaign sidecars exist", async () => {
  const value = await fixture();
  try {
    value.packageReceiptSha256 = "e".repeat(64);
    await writeCampaignSidecars(value);
    await rejectsCode(
      () =>
        validate(value, {
          expectedPackageReceiptSha256: "e".repeat(64),
          expectedCampaignReceiptSha256: "f".repeat(64),
        }),
      "package-missing",
    );
  } finally {
    await close(value);
  }
});

test("package-unsigned still wins before campaign codes", async () => {
  const value = await fixture();
  try {
    await writePackageSidecars(value, { authenticode: false });
    const campaign = await writeCampaignSidecars(value);
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: sha256(campaign.receiptBytes),
        }),
      "package-unsigned",
    );
  } finally {
    await close(value);
  }
});

test("stale expected campaign receipt hash is campaign-stale", async () => {
  const value = await fixture();
  try {
    await bound(value);
    await rejectsCode(
      () => validate(value, { expectedCampaignReceiptSha256: "e".repeat(64) }),
      "campaign-stale",
    );
  } finally {
    await close(value);
  }
});

test("stale sourceCommit on the campaign receipt is campaign-stale", async () => {
  const value = await fixture();
  try {
    await writePackageSidecars(value);
    const campaign = await writeCampaignSidecars(value, {
      receipt: { sourceCommit: "e".repeat(40) },
    });
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: sha256(campaign.receiptBytes),
        }),
      "campaign-stale",
    );
  } finally {
    await close(value);
  }
});

test("stale sourceCommit on the campaign admission sidecar is campaign-stale", async () => {
  const value = await fixture();
  try {
    await writePackageSidecars(value);
    const campaign = await writeCampaignSidecars(value, {
      admission: { sourceCommit: "e".repeat(40) },
    });
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: sha256(campaign.receiptBytes),
        }),
      "campaign-stale",
    );
  } finally {
    await close(value);
  }
});

test("admission versus receipt sourceCommit disagreement is campaign-stale", async () => {
  const value = await fixture();
  try {
    await writePackageSidecars(value);
    const campaign = await writeCampaignSidecars(value, {
      receipt: { sourceCommit: "e".repeat(40) },
      admission: { sourceCommit: "f".repeat(40) },
    });
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: sha256(campaign.receiptBytes),
        }),
      "campaign-stale",
    );
  } finally {
    await close(value);
  }
});

test("admission packageReceiptSha256 disagreeing with the expected package receipt is campaign-stale", async () => {
  const value = await fixture();
  try {
    await writePackageSidecars(value);
    const campaign = await writeCampaignSidecars(value, {
      admission: { packageReceiptSha256: "e".repeat(64) },
    });
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: sha256(campaign.receiptBytes),
        }),
      "campaign-stale",
    );
  } finally {
    await close(value);
  }
});

test("receipt packageReceiptSha256 disagreeing with the expected package receipt is campaign-stale", async () => {
  const value = await fixture();
  try {
    await writePackageSidecars(value);
    const campaign = await writeCampaignSidecars(value, {
      receipt: { packageReceiptSha256: "e".repeat(64) },
    });
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: sha256(campaign.receiptBytes),
        }),
      "campaign-stale",
    );
  } finally {
    await close(value);
  }
});

test("receipt bytes that disagree with declared campaignReceiptSha256 are campaign-substituted", async () => {
  const value = await fixture();
  try {
    await writePackageSidecars(value);
    const campaign = await writeCampaignSidecars(value);
    await writeFile(
      path.join(value.root, "campaign-receipt.json"),
      Buffer.concat([campaign.receiptBytes, Buffer.from(" ")]),
    );
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: sha256(campaign.receiptBytes),
        }),
      "campaign-substituted",
    );
  } finally {
    await close(value);
  }
});

test("journey file bytes that disagree with declared sha256 are campaign-substituted", async () => {
  const value = await fixture();
  try {
    const boundValue = await bound(value);
    for (const id of quartet) {
      const file = path.join(value.root, `${id}.json`);
      const bytes = await readFile(file);
      await writeFile(file, Buffer.concat([bytes, Buffer.from(" ")]));
      await rejectsCode(
        () =>
          validate(value, {
            expectedCampaignReceiptSha256: boundValue.campaignReceiptSha256,
          }),
        "campaign-substituted",
      );
      await writeFile(file, bytes);
    }
    await writeFile(
      path.join(value.root, "acp05-renderer-pairing-receipt.json"),
      Buffer.concat([boundValue.rendererPairingReceiptBytes, Buffer.from(" ")]),
    );
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: boundValue.campaignReceiptSha256,
        }),
      "campaign-substituted",
    );
  } finally {
    await close(value);
  }
});

test("admission installerSha256 disagreeing with expected is campaign-substituted", async () => {
  const value = await fixture();
  try {
    await writePackageSidecars(value);
    const campaign = await writeCampaignSidecars(value, {
      admission: { installerSha256: "e".repeat(64) },
    });
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: sha256(campaign.receiptBytes),
        }),
      "campaign-substituted",
    );
  } finally {
    await close(value);
  }
});

test("admission installerSha256 disagreeing with the receipt is campaign-substituted", async () => {
  const value = await fixture();
  try {
    await writePackageSidecars(value);
    const campaign = await writeCampaignSidecars(value, {
      admission: { installerSha256: "e".repeat(64) },
      receipt: { installerSha256: "f".repeat(64) },
    });
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: sha256(campaign.receiptBytes),
        }),
      "campaign-substituted",
    );
  } finally {
    await close(value);
  }
});

test("receipt installerSha256 disagreeing with expected is campaign-substituted", async () => {
  const value = await fixture();
  try {
    await writePackageSidecars(value);
    const campaign = await writeCampaignSidecars(value, {
      receipt: { installerSha256: "e".repeat(64) },
    });
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: sha256(campaign.receiptBytes),
        }),
      "campaign-substituted",
    );
  } finally {
    await close(value);
  }
});

test("extra or missing keys on either campaign sidecar are schema-invalid", async () => {
  const value = await fixture();
  try {
    await writePackageSidecars(value);
    const extraAdmission = await writeCampaignSidecars(value, {
      admission: { extra: true },
    });
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: sha256(extraAdmission.receiptBytes),
        }),
      "schema-invalid",
    );
    const extraReceipt = await writeCampaignSidecars(value, {
      receipt: { extra: true },
    });
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: sha256(extraReceipt.receiptBytes),
        }),
      "schema-invalid",
    );
    const missingAdmission = await writeCampaignSidecars(value);
    const admission = JSON.parse(missingAdmission.admissionBytes);
    delete admission.installerSha256;
    await writeFile(
      path.join(value.root, "campaign-admission.json"),
      JSON.stringify(admission),
    );
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: sha256(missingAdmission.receiptBytes),
        }),
      "schema-invalid",
    );
    const missingReceipt = await writeCampaignSidecars(value);
    const receipt = JSON.parse(missingReceipt.receiptBytes);
    delete receipt.campaignKind;
    await writeFile(
      path.join(value.root, "campaign-receipt.json"),
      JSON.stringify(receipt),
    );
    await writeFile(
      path.join(value.root, "campaign-admission.json"),
      JSON.stringify({
        ...JSON.parse(missingReceipt.admissionBytes),
        campaignReceiptSha256: sha256(Buffer.from(JSON.stringify(receipt))),
      }),
    );
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: sha256(
            Buffer.from(JSON.stringify(receipt)),
          ),
        }),
      "schema-invalid",
    );
  } finally {
    await close(value);
  }
});

test("extra or missing journeyEvidence item keys are schema-invalid", async () => {
  const value = await fixture();
  try {
    await writePackageSidecars(value);
    const items = await journeyEvidenceFor(value.root);
    const extra = await writeCampaignSidecars(value, {
      journeyEvidence: items.map((item, index) =>
        index === 0 ? { ...item, extra: true } : item,
      ),
    });
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: sha256(extra.receiptBytes),
        }),
      "schema-invalid",
    );
    const missing = await writeCampaignSidecars(value, {
      journeyEvidence: items.map((item, index) =>
        index === 1 ? { id: item.id } : item,
      ),
    });
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: sha256(missing.receiptBytes),
        }),
      "schema-invalid",
    );
  } finally {
    await close(value);
  }
});

test("wrong campaignKind is schema-invalid", async () => {
  const value = await fixture();
  try {
    await writePackageSidecars(value);
    const campaign = await writeCampaignSidecars(value, {
      receipt: { campaignKind: "security-only" },
    });
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: sha256(campaign.receiptBytes),
        }),
      "schema-invalid",
    );
  } finally {
    await close(value);
  }
});

test("wrong id in each quartet slot is schema-invalid", async () => {
  const value = await fixture();
  try {
    await writePackageSidecars(value);
    const items = await journeyEvidenceFor(value.root);
    for (let index = 0; index < quartet.length; index += 1) {
      const journeyEvidence = items.map((item, slot) =>
        slot === index ? { ...item, id: "git.basic" } : item,
      );
      const campaign = await writeCampaignSidecars(value, { journeyEvidence });
      await rejectsCode(
        () =>
          validate(value, {
            expectedCampaignReceiptSha256: sha256(campaign.receiptBytes),
          }),
        "schema-invalid",
      );
    }
  } finally {
    await close(value);
  }
});

test("journeyEvidence count other than 4 is schema-invalid", async () => {
  const value = await fixture();
  try {
    await writePackageSidecars(value);
    const items = await journeyEvidenceFor(value.root);
    for (const journeyEvidence of [items.slice(0, 3), [...items, items[0]]]) {
      const campaign = await writeCampaignSidecars(value, { journeyEvidence });
      await rejectsCode(
        () =>
          validate(value, {
            expectedCampaignReceiptSha256: sha256(campaign.receiptBytes),
          }),
        "schema-invalid",
      );
    }
  } finally {
    await close(value);
  }
});

test("wrong-shaped commit or hash on every sidecar field is schema-invalid", async () => {
  const value = await fixture();
  try {
    await writePackageSidecars(value);
    const cases = [
      { admission: { sourceCommit: "a".repeat(39) } },
      { receipt: { sourceCommit: "a".repeat(39) } },
      { admission: { packageReceiptSha256: "c".repeat(63) } },
      { receipt: { packageReceiptSha256: "c".repeat(63) } },
      { admission: { installerSha256: "c".repeat(63) } },
      { receipt: { installerSha256: "c".repeat(63) } },
      { receipt: { rendererPairingReceiptSha256: "c".repeat(63) } },
      { admission: { contractSha256: "c".repeat(63) } },
      { admission: { campaignReceiptSha256: "c".repeat(63) } },
    ];
    for (const override of cases) {
      const campaign = await writeCampaignSidecars(value, override);
      await rejectsCode(
        () =>
          validate(value, {
            expectedCampaignReceiptSha256: sha256(campaign.receiptBytes),
          }),
        "schema-invalid",
      );
    }
    const items = await journeyEvidenceFor(value.root);
    const journey = await writeCampaignSidecars(value, {
      journeyEvidence: items.map((item, index) =>
        index === 2 ? { ...item, sha256: "c".repeat(63) } : item,
      ),
    });
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: sha256(journey.receiptBytes),
        }),
      "schema-invalid",
    );
  } finally {
    await close(value);
  }
});

test("wrong schemaVersion or workItem on either sidecar is schema-invalid", async () => {
  const value = await fixture();
  try {
    await writePackageSidecars(value);
    for (const override of [
      { admission: { schemaVersion: 2 } },
      { receipt: { schemaVersion: 2 } },
      { admission: { workItem: "OTHER" } },
      { receipt: { workItem: "OTHER" } },
    ]) {
      const campaign = await writeCampaignSidecars(value, override);
      await rejectsCode(
        () =>
          validate(value, {
            expectedCampaignReceiptSha256: sha256(campaign.receiptBytes),
          }),
        "schema-invalid",
      );
    }
  } finally {
    await close(value);
  }
});

test("wrong-shaped expectedCampaignReceiptSha256 is schema-invalid", async () => {
  const value = await fixture();
  try {
    await writePackageSidecars(value);
    await rejectsCode(
      () => validate(value, { expectedCampaignReceiptSha256: "not-a-hash" }),
      "schema-invalid",
    );
  } finally {
    await close(value);
  }
});

test("admission contractSha256 drift is contract-drift", async () => {
  const value = await fixture();
  try {
    await writePackageSidecars(value);
    const campaign = await writeCampaignSidecars(value, {
      admission: { contractSha256: "e".repeat(64) },
    });
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: sha256(campaign.receiptBytes),
        }),
      "contract-drift",
    );
  } finally {
    await close(value);
  }
});

test("sidecar byte ceilings accept 65536 and reject 65537", async () => {
  const exact = await fixture();
  try {
    await writePackageSidecars(exact);
    const initial = await writeCampaignSidecars(exact);
    const padding = 65_536 - initial.receiptBytes.length;
    const padded = await writeCampaignSidecars(exact, {
      receiptPadding: padding,
    });
    assert.equal(
      (
        await validate(exact, {
          expectedCampaignReceiptSha256: sha256(padded.receiptBytes),
        })
      ).verdict,
      "passed",
    );
  } finally {
    await close(exact);
  }
  const plus = await fixture();
  try {
    await writePackageSidecars(plus);
    const initial = await writeCampaignSidecars(plus);
    await writeCampaignSidecars(plus, {
      receiptPadding: 65_537 - initial.receiptBytes.length,
    });
    await rejectsCode(
      () => validate(plus, { expectedCampaignReceiptSha256: "e".repeat(64) }),
      "evidence-oversize",
    );
  } finally {
    await close(plus);
  }
});

test("malformed campaign sidecar JSON is malformed-json", async () => {
  const value = await fixture();
  try {
    const boundValue = await bound(value);
    await writeFile(path.join(value.root, "campaign-receipt.json"), "{");
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: boundValue.campaignReceiptSha256,
        }),
      "malformed-json",
    );
  } finally {
    await close(value);
  }
});

test("campaign sidecar junction or symlink is evidence-link", async () => {
  const value = await fixture();
  try {
    const boundValue = await bound(value);
    const receipt = path.join(value.root, "campaign-receipt.json");
    const admission = path.join(value.root, "campaign-admission.json");
    await unlink(receipt);
    await symlink(admission, receipt);
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: boundValue.campaignReceiptSha256,
        }),
      "evidence-link",
    );
  } finally {
    await close(value);
  }
});

test("campaign sidecar identity replacement during read is evidence-identity-changed", async () => {
  const value = await fixture();
  try {
    const boundValue = await bound(value);
    const file = path.join(value.root, "campaign-admission.json");
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: boundValue.campaignReceiptSha256,
          onAfterCampaignOpen: async () => {
            await unlink(file);
            await writeFile(file, boundValue.admissionBytes);
          },
        }),
      "evidence-identity-changed",
    );
  } finally {
    await close(value);
  }
});

test("journey bytes that change between the two reads are evidence-identity-changed", async () => {
  const value = await fixture();
  try {
    const boundValue = await bound(value);
    const file = path.join(value.root, `${quartet[0]}.json`);
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: boundValue.campaignReceiptSha256,
          onAfterFirstJourneyBind: async () => {
            const bytes = await readFile(file);
            await writeFile(file, Buffer.concat([bytes, Buffer.from(" ")]));
          },
        }),
      "evidence-identity-changed",
    );
  } finally {
    await close(value);
  }
});

test("journey file oversize on re-read is evidence-oversize", async () => {
  const value = await fixture();
  try {
    const boundValue = await bound(value);
    const file = path.join(value.root, `${quartet[0]}.json`);
    const bytes = await readFile(file);
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: boundValue.campaignReceiptSha256,
          onAfterCampaignOpen: async () => {
            await writeFile(
              file,
              Buffer.concat([bytes, Buffer.alloc(1_048_577 - bytes.length)]),
            );
          },
        }),
      "evidence-oversize",
    );
  } finally {
    await close(value);
  }
});

test("journey file junction on re-read is evidence-link", async () => {
  const value = await fixture();
  try {
    const boundValue = await bound(value);
    const file = path.join(value.root, `${quartet[0]}.json`);
    const other = path.join(value.root, `${quartet[1]}.json`);
    let campaignOpen = 0;
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: boundValue.campaignReceiptSha256,
          onAfterCampaignOpen: async () => {
            campaignOpen += 1;
            if (campaignOpen !== 3) return;
            await unlink(file);
            await link(other, file);
          },
        }),
      "evidence-link",
    );
  } finally {
    await close(value);
  }
});

test("journey identity replacement during read is evidence-identity-changed", async () => {
  const value = await fixture();
  try {
    const boundValue = await bound(value);
    const file = path.join(value.root, `${quartet[0]}.json`);
    const bytes = await readFile(file);
    await rejectsCode(
      () =>
        validate(value, {
          expectedCampaignReceiptSha256: boundValue.campaignReceiptSha256,
          onAfterJourneyOpen: async () => {
            await unlink(file);
            await writeFile(file, bytes);
          },
        }),
      "evidence-identity-changed",
    );
  } finally {
    await close(value);
  }
});

test("failed S6 verdict still requires campaign authentication", async () => {
  const missing = await fixture({
    statuses: { "git.basic": "failed" },
    aggregate: { verdict: "failed" },
  });
  try {
    const { packageReceiptSha256 } = await writePackageSidecars(missing);
    await rejectsCode(
      () =>
        validate(missing, {
          expectedPackageReceiptSha256: packageReceiptSha256,
          expectedCampaignReceiptSha256: "e".repeat(64),
        }),
      "campaign-missing",
    );
  } finally {
    await close(missing);
  }
  const boundFailed = await fixture({
    statuses: { "git.basic": "failed" },
    aggregate: { verdict: "failed" },
  });
  try {
    const boundValue = await bound(boundFailed);
    assert.equal(
      (
        await validate(boundFailed, {
          expectedCampaignReceiptSha256: boundValue.campaignReceiptSha256,
        })
      ).verdict,
      "failed",
    );
  } finally {
    await close(boundFailed);
  }
});

test("incomplete S6 verdict still requires campaign authentication", async () => {
  const missing = await fixture({
    omit: ["git.basic"],
    aggregate: { verdict: "incomplete" },
  });
  try {
    const { packageReceiptSha256 } = await writePackageSidecars(missing);
    await rejectsCode(
      () =>
        validate(missing, {
          expectedPackageReceiptSha256: packageReceiptSha256,
          expectedCampaignReceiptSha256: "e".repeat(64),
        }),
      "campaign-missing",
    );
  } finally {
    await close(missing);
  }
  const boundIncomplete = await fixture({
    omit: ["git.basic"],
    aggregate: { verdict: "incomplete" },
  });
  try {
    const boundValue = await bound(boundIncomplete);
    assert.equal(
      (
        await validate(boundIncomplete, {
          expectedCampaignReceiptSha256: boundValue.campaignReceiptSha256,
        })
      ).verdict,
      "incomplete",
    );
  } finally {
    await close(boundIncomplete);
  }
});

test("campaign codes never become incomplete or passed", async () => {
  const missing = await fixture();
  try {
    const { packageReceiptSha256 } = await writePackageSidecars(missing);
    await rejectsCode(
      () =>
        validate(missing, {
          expectedPackageReceiptSha256: packageReceiptSha256,
          expectedCampaignReceiptSha256: "e".repeat(64),
        }),
      "campaign-missing",
    );
  } finally {
    await close(missing);
  }
  const stale = await fixture();
  try {
    await bound(stale);
    await rejectsCode(
      () => validate(stale, { expectedCampaignReceiptSha256: "e".repeat(64) }),
      "campaign-stale",
    );
  } finally {
    await close(stale);
  }
});
