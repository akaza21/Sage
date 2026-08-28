import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
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

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

function fixtureAuthenticodeCatalog() {
  const admittedThumbprints = new Set([signerThumbprint]);
  return {
    verify: (thumbprint) => admittedThumbprints.has(thumbprint),
  };
}

async function packageAdmission() {
  return import("./beta0001-candidate-package.mjs");
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

async function fixture() {
  const root = await mkdtemp(
    path.join(os.tmpdir(), "beta0001-candidate-package-"),
  );
  const { value: threshold, sha256: contractSha256 } =
    await thresholdAuthority();
  const definitions = metricDefinitions(threshold);
  const evidence = ids.map((id) => ({ id, status: "passed" }));
  for (const { id } of evidence) {
    const metrics = Object.fromEntries(definitions.get(id) ?? []);
    await writeFile(
      path.join(root, `${id}.json`),
      JSON.stringify({
        schemaVersion: 1,
        evidenceId: id,
        contractSha256,
        candidate: candidate(),
        status: "passed",
        metrics,
      }),
    );
  }
  const result = {
    schemaVersion: 1,
    workItem: "BETA-0001",
    contractSha256,
    candidate: candidate(),
    evidence,
    verdict: "passed",
  };
  return {
    root,
    contractSha256,
    resultBytes: Buffer.from(JSON.stringify(result)),
    authenticodeCatalog: fixtureAuthenticodeCatalog(),
  };
}

async function writeSidecars(
  value,
  {
    receipt = {},
    admission = {},
    receiptPadding = 0,
    admissionPadding = 0,
  } = {},
) {
  const receiptValue = {
    schemaVersion: 1,
    workItem: "BETA-0001",
    sourceCommit,
    installerSha256,
    installedExecutableSha256,
    signerThumbprint,
    authenticode: true,
    ...receipt,
  };
  const receiptBytes = Buffer.concat([
    Buffer.from(JSON.stringify(receiptValue)),
    Buffer.alloc(receiptPadding, 0x20),
  ]);
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
  const admissionBytes = Buffer.concat([
    Buffer.from(JSON.stringify(admissionValue)),
    Buffer.alloc(admissionPadding, 0x20),
  ]);
  await writeFile(
    path.join(value.root, "package-admission.json"),
    admissionBytes,
  );
  return { receiptBytes, admissionBytes };
}

async function validate(value, options = {}) {
  const { validateBetaCandidatePackageAdmission } = await packageAdmission();
  return validateBetaCandidatePackageAdmission({
    resultBytes: value.resultBytes,
    evidenceRoot: value.root,
    expectedSourceCommit: sourceCommit,
    expectedPackageReceiptSha256:
      options.expectedPackageReceiptSha256 ?? "e".repeat(64),
    expectedInstallerSha256: installerSha256,
    authenticodeCatalog:
      options.authenticodeCatalog ?? value.authenticodeCatalog,
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

test("signed package admission returns the matrix verdict", async () => {
  const value = await fixture();
  try {
    const { receiptBytes } = await writeSidecars(value);
    assert.equal(
      (
        await validate(value, {
          expectedPackageReceiptSha256: sha256(receiptBytes),
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
    const { receiptBytes } = await writeSidecars(value);
    value.authenticodeCatalog = undefined;
    await rejectsCode(
      () =>
        validate(value, {
          expectedPackageReceiptSha256: sha256(receiptBytes),
        }),
      "package-unsigned",
    );
  } finally {
    await close(value);
  }
});

test("s5-only fixture without package sidecars is package-missing", async () => {
  const value = await fixture();
  try {
    await rejectsCode(() => validate(value), "package-missing");
  } finally {
    await close(value);
  }
});

test("missing package-receipt sidecar is package-missing", async () => {
  const value = await fixture();
  try {
    await writeSidecars(value);
    await rm(path.join(value.root, "package-receipt.json"));
    await rejectsCode(() => validate(value), "package-missing");
  } finally {
    await close(value);
  }
});

test("unsigned authenticode flag is package-unsigned", async () => {
  const value = await fixture();
  try {
    const { receiptBytes } = await writeSidecars(value, {
      receipt: { authenticode: false },
    });
    await rejectsCode(
      () =>
        validate(value, { expectedPackageReceiptSha256: sha256(receiptBytes) }),
      "package-unsigned",
    );
  } finally {
    await close(value);
  }
});

test("denying authenticode catalog is package-unsigned", async () => {
  const value = await fixture();
  try {
    const { receiptBytes } = await writeSidecars(value);
    await rejectsCode(
      () =>
        validate(value, {
          expectedPackageReceiptSha256: sha256(receiptBytes),
          authenticodeCatalog: { verify: () => false },
        }),
      "package-unsigned",
    );
  } finally {
    await close(value);
  }
});

test("catalog verify receives the receipt signerThumbprint", async () => {
  const value = await fixture();
  try {
    const { receiptBytes } = await writeSidecars(value);
    let received;
    await validate(value, {
      expectedPackageReceiptSha256: sha256(receiptBytes),
      authenticodeCatalog: {
        verify: (thumbprint) => {
          received = thumbprint;
          return true;
        },
      },
    });
    assert.equal(received, signerThumbprint);
    await rejectsCode(
      () =>
        validate(value, {
          expectedPackageReceiptSha256: sha256(receiptBytes),
          authenticodeCatalog: {
            verify: (thumbprint) => thumbprint === "f".repeat(40),
          },
        }),
      "package-unsigned",
    );
  } finally {
    await close(value);
  }
});

test("stale expected receipt hash is package-stale", async () => {
  const value = await fixture();
  try {
    await writeSidecars(value);
    await rejectsCode(() => validate(value), "package-stale");
  } finally {
    await close(value);
  }
});

test("stale sourceCommit on the receipt is package-stale", async () => {
  const value = await fixture();
  try {
    const { receiptBytes } = await writeSidecars(value, {
      receipt: { sourceCommit: "e".repeat(40) },
    });
    const receipt = JSON.parse(receiptBytes);
    await writeSidecars(value, {
      receipt,
      admission: { packageReceiptSha256: sha256(receiptBytes) },
    });
    await rejectsCode(
      () =>
        validate(value, { expectedPackageReceiptSha256: sha256(receiptBytes) }),
      "package-stale",
    );
  } finally {
    await close(value);
  }
});

test("stale sourceCommit on the admission sidecar is package-stale", async () => {
  const value = await fixture();
  try {
    const { receiptBytes } = await writeSidecars(value, {
      admission: { sourceCommit: "e".repeat(40) },
    });
    await rejectsCode(
      () =>
        validate(value, { expectedPackageReceiptSha256: sha256(receiptBytes) }),
      "package-stale",
    );
  } finally {
    await close(value);
  }
});

test("admission versus receipt sourceCommit disagreement is package-stale", async () => {
  const value = await fixture();
  try {
    const { receiptBytes } = await writeSidecars(value, {
      admission: { sourceCommit: "e".repeat(40) },
    });
    await rejectsCode(
      () =>
        validate(value, { expectedPackageReceiptSha256: sha256(receiptBytes) }),
      "package-stale",
    );
  } finally {
    await close(value);
  }
});

test("receipt bytes that disagree with declared packageReceiptSha256 are package-substituted", async () => {
  const value = await fixture();
  try {
    await writeSidecars(value, {
      admission: { packageReceiptSha256: "e".repeat(64) },
    });
    await rejectsCode(
      () => validate(value, { expectedPackageReceiptSha256: "e".repeat(64) }),
      "package-substituted",
    );
  } finally {
    await close(value);
  }
});

test("installer hash disagreement is package-substituted", async () => {
  const value = await fixture();
  try {
    const { receiptBytes } = await writeSidecars(value, {
      receipt: { installerSha256: "e".repeat(64) },
    });
    await rejectsCode(
      () =>
        validate(value, { expectedPackageReceiptSha256: sha256(receiptBytes) }),
      "package-substituted",
    );
  } finally {
    await close(value);
  }
});

test("extra or missing package keys are schema-invalid", async () => {
  for (const change of [
    { receipt: { extra: true } },
    { admission: { installerSha256: undefined } },
  ]) {
    const value = await fixture();
    try {
      const { receiptBytes } = await writeSidecars(value, change);
      await rejectsCode(
        () =>
          validate(value, {
            expectedPackageReceiptSha256: sha256(receiptBytes),
          }),
        "schema-invalid",
      );
    } finally {
      await close(value);
    }
  }
});

test("wrong-shaped commit hash or thumbprint is schema-invalid", async () => {
  for (const receipt of [
    { sourceCommit: "A".repeat(40) },
    { signerThumbprint: "b".repeat(39) },
  ]) {
    const value = await fixture();
    try {
      const { receiptBytes } = await writeSidecars(value, { receipt });
      await rejectsCode(
        () =>
          validate(value, {
            expectedPackageReceiptSha256: sha256(receiptBytes),
          }),
        "schema-invalid",
      );
    } finally {
      await close(value);
    }
  }
});

test("admission contractSha256 drift is contract-drift", async () => {
  const value = await fixture();
  try {
    const { receiptBytes } = await writeSidecars(value, {
      admission: { contractSha256: "e".repeat(64) },
    });
    await rejectsCode(
      () =>
        validate(value, { expectedPackageReceiptSha256: sha256(receiptBytes) }),
      "contract-drift",
    );
  } finally {
    await close(value);
  }
});

test("sidecar byte ceilings accept 65536 and reject 65537", async () => {
  const exact = await fixture();
  try {
    const initial = await writeSidecars(exact);
    const padding = 65_536 - initial.receiptBytes.length;
    const { receiptBytes } = await writeSidecars(exact, {
      receiptPadding: padding,
    });
    assert.equal(
      (
        await validate(exact, {
          expectedPackageReceiptSha256: sha256(receiptBytes),
        })
      ).verdict,
      "passed",
    );
  } finally {
    await close(exact);
  }
  const plus = await fixture();
  try {
    const initial = await writeSidecars(plus);
    await writeSidecars(plus, {
      receiptPadding: 65_537 - initial.receiptBytes.length,
    });
    await rejectsCode(() => validate(plus), "evidence-oversize");
  } finally {
    await close(plus);
  }
});

test("package codes never become incomplete or passed", async () => {
  const missing = await fixture();
  try {
    await rejectsCode(() => validate(missing), "package-missing");
  } finally {
    await close(missing);
  }
  const unsigned = await fixture();
  try {
    const { receiptBytes } = await writeSidecars(unsigned, {
      receipt: { authenticode: false },
    });
    await rejectsCode(
      () =>
        validate(unsigned, {
          expectedPackageReceiptSha256: sha256(receiptBytes),
        }),
      "package-unsigned",
    );
  } finally {
    await close(unsigned);
  }
});
