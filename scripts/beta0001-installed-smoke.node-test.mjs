import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import {
  INSTALLED_SMOKE_STEP_IDS,
  InstalledSmokeOperationError,
  runInstalledNsisSmoke,
  validateInstalledSmokeReceipt,
} from "./beta0001-installed-smoke.mjs";
import { runAuthenticatedInstalledSmoke } from "./beta0001-installed-smoke-handoff.mjs";

const SHA = "a".repeat(64);

function request() {
  return {
    campaignId: "beta-candidate-01",
    packageReceiptSha256: "b".repeat(64),
    installer: {
      kind: "nsis",
      sha256: "c".repeat(64),
      bytes: 4096,
      expectedInstalledExecutableSha256: "1".repeat(64),
      expectedInstalledExecutableBytes: 8192,
      signerThumbprint: "f".repeat(40),
    },
    fixture: {
      rootIdentitySha256: "d".repeat(64),
      rootLabel: "sage-installed-smoke-fixture",
      initialCommitSha256: "e".repeat(64),
    },
  };
}

function prerequisites(overrides = {}) {
  return {
    windows: true,
    interactiveSession: true,
    webView2: true,
    administrator: true,
    authenticode: true,
    git: true,
    osVersion: "10.0.26100",
    webView2Version: "140.0.3485.66",
    gitVersion: "2.50.0.windows.1",
    signerThumbprint: "f".repeat(40),
    ...overrides,
  };
}

test("installed NSIS smoke runs the exact lifecycle and validates its bounded receipt", async () => {
  const calls = [];
  const receipt = await runInstalledNsisSmoke({
    request: request(),
    clock: (() => {
      let tick = Date.parse("2026-08-11T00:00:00.000Z");
      return () => new Date((tick += 25));
    })(),
    operations: {
      inspectPrerequisites: () => prerequisites(),
      runStep: (id) => {
        calls.push(id);
        return {
          durationMs: 25,
          evidenceSha256: SHA,
          assertions:
            id === "drive-primary"
              ? {
                  projectSeeded: true,
                  fileOpened: true,
                  fileEditedAndSaved: true,
                  workspaceSearchMatched: true,
                  gitStaged: true,
                  gitCommitted: true,
                  chatCreated: true,
                  chatRenamed: true,
                  chatSwitched: true,
                  simulatorTurnCompleted: true,
                }
              : id === "verify-restart"
                ? { restartPersisted: true }
                : id === "verify-reinstall"
                  ? { reinstallVerified: true }
                  : {},
        };
      },
    },
  });

  assert.deepEqual(calls, INSTALLED_SMOKE_STEP_IDS);
  assert.equal(receipt.result, "passed");
  assert.equal(receipt.steps.length, INSTALLED_SMOKE_STEP_IDS.length);
  assert.equal(receipt.assertions.gitCommitted, true);
  assert.deepEqual(validateInstalledSmokeReceipt(receipt), []);
});

test("missing signed interactive prerequisites fail closed before mutation", async () => {
  let mutations = 0;
  const receipt = await runInstalledNsisSmoke({
    request: request(),
    clock: () => new Date("2026-08-11T00:00:00.000Z"),
    operations: {
      inspectPrerequisites: () =>
        prerequisites({ authenticode: false, signerThumbprint: null }),
      runStep: () => {
        mutations += 1;
        throw new Error("must not run");
      },
    },
  });

  assert.equal(mutations, 0);
  assert.equal(receipt.result, "blocked");
  assert.equal(receipt.failure.code, "installer-signature-invalid");
  assert.ok(receipt.steps.every(({ status }) => status === "skipped"));
  assert.deepEqual(validateInstalledSmokeReceipt(receipt), []);
});

test("receipt validation rejects omitted lifecycle evidence and unbounded strings", async () => {
  const receipt = await runInstalledNsisSmoke({
    request: request(),
    operations: {
      inspectPrerequisites: () => prerequisites(),
      runStep: (id) => ({
        durationMs: 1,
        evidenceSha256: SHA,
        assertions:
          id === "drive-primary"
            ? Object.fromEntries(
                [
                  "projectSeeded",
                  "fileOpened",
                  "fileEditedAndSaved",
                  "workspaceSearchMatched",
                  "gitStaged",
                  "gitCommitted",
                  "chatCreated",
                  "chatRenamed",
                  "chatSwitched",
                  "simulatorTurnCompleted",
                ].map((key) => [key, true]),
              )
            : id === "verify-restart"
              ? { restartPersisted: true }
              : id === "verify-reinstall"
                ? { reinstallVerified: true }
                : {},
      }),
    },
  });
  receipt.steps.pop();
  receipt.prerequisites.gitVersion = "g".repeat(1025);
  assert.deepEqual(validateInstalledSmokeReceipt(receipt), [
    "receipt.prerequisites.gitVersion exceeds 128 UTF-8 bytes",
    "receipt.steps must contain the exact installed-smoke lifecycle",
  ]);
});

test("typed process timeout preserves failure and runs only authoritative cleanup", async () => {
  const calls = [];
  const receipt = await runInstalledNsisSmoke({
    request: request(),
    operations: {
      inspectPrerequisites: () => prerequisites(),
      runStep: (id) => {
        calls.push(id);
        if (id === "install-initial") {
          throw new InstalledSmokeOperationError("step-timeout");
        }
        return { durationMs: 4, evidenceSha256: SHA, assertions: {} };
      },
    },
  });

  assert.deepEqual(calls, ["install-initial", "uninstall-final"]);
  assert.equal(receipt.result, "failed");
  assert.equal(receipt.failure.code, "step-timeout");
  assert.equal(receipt.steps.at(-1).status, "passed");
  assert.deepEqual(validateInstalledSmokeReceipt(receipt), []);
});

test("authenticated handoff binds signer and installed executable identity into the runner", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-installed-handoff-"));
  try {
    const installerPath = path.join(root, "sage-setup.exe");
    const installedPath = path.join(root, "sage.exe");
    writeFileSync(installerPath, "installer");
    writeFileSync(installedPath, "installed");
    const smokeRequest = request();
    smokeRequest.installer.bytes = 9;
    smokeRequest.installer.expectedInstalledExecutableBytes = 9;
    const receipt = runInstalledNsisSmoke({
      request: smokeRequest,
      operations: {
        inspectPrerequisites: () => prerequisites(),
        runStep: (id) => ({
          durationMs: 1,
          evidenceSha256: SHA,
          assertions:
            id === "drive-primary"
              ? Object.fromEntries(
                  assertionKeysForTest().map((key) => [key, true]),
                )
              : id === "verify-restart"
                ? { restartPersisted: true }
                : id === "verify-reinstall"
                  ? { reinstallVerified: true }
                  : {},
        }),
      },
    });
    const handoff = {
      receiptBytes: Buffer.from("package-receipt"),
      receipt: {
        participants: {
          candidate: {
            installers: [
              {
                kind: "nsis",
                installedExecutable: {
                  sha256:
                    smokeRequest.installer.expectedInstalledExecutableSha256,
                  bytes: 9,
                },
              },
            ],
          },
        },
      },
      artifacts: {
        candidate: {
          installerArtifacts: {
            nsis: {
              path: installerPath,
              sha256: smokeRequest.installer.sha256,
              bytes: 9,
            },
          },
          installedExecutables: {
            nsis: {
              path: installedPath,
              sha256: smokeRequest.installer.expectedInstalledExecutableSha256,
              bytes: 9,
            },
          },
        },
      },
      tools: {
        git: { path: process.execPath, sha256: "2".repeat(64), bytes: 1 },
      },
    };
    let args;
    let processCalls = 0;
    const result = await runAuthenticatedInstalledSmoke({
      handoff,
      campaignId: smokeRequest.campaignId,
      outputDirectory: path.join(root, "output"),
      powershell: process.execPath,
      node: process.execPath,
      expectedSignerThumbprint: smokeRequest.installer.signerThumbprint,
      sourceRoot: path.resolve("."),
      runProcess: async (_executable, received) => {
        processCalls += 1;
        args = received;
        const output = received[received.indexOf("-OutputDirectory") + 1];
        receipt.packageReceiptSha256 = "placeholder";
        const expectedPackage =
          received[received.indexOf("-PackageReceiptSha256") + 1];
        receipt.packageReceiptSha256 = expectedPackage;
        writeFileSync(
          path.join(output, "receipt.json"),
          `${JSON.stringify(receipt)}\n`,
        );
      },
    });
    assert.equal(result.result, "passed");
    assert.equal(
      args[args.indexOf("-ExpectedInstalledExecutableSha256") + 1],
      smokeRequest.installer.expectedInstalledExecutableSha256,
    );
    assert.equal(
      args[args.indexOf("-ExpectedSignerThumbprint") + 1],
      smokeRequest.installer.signerThumbprint,
    );
    assert.equal(args[args.indexOf("-GitSha256") + 1], "2".repeat(64));
    assert.equal(args[args.indexOf("-GitBytes") + 1], "1");
    handoff.artifacts.candidate.installedExecutables.nsis.sha256 = "9".repeat(
      64,
    );
    await assert.rejects(
      runAuthenticatedInstalledSmoke({
        handoff,
        campaignId: smokeRequest.campaignId,
        outputDirectory: path.join(root, "blocked-output"),
        powershell: process.execPath,
        node: process.execPath,
        expectedSignerThumbprint: smokeRequest.installer.signerThumbprint,
        sourceRoot: path.resolve("."),
        runProcess: async () => {
          processCalls += 1;
        },
      }),
      /installed-smoke-authenticated-nsis-binding-invalid/,
    );
    assert.equal(processCalls, 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

function assertionKeysForTest() {
  return [
    "projectSeeded",
    "fileOpened",
    "fileEditedAndSaved",
    "workspaceSearchMatched",
    "gitStaged",
    "gitCommitted",
    "chatCreated",
    "chatRenamed",
    "chatSwitched",
    "simulatorTurnCompleted",
  ];
}

test("real runner gates mutation on the configured Authenticode signer", () => {
  const source = readFileSync(
    new URL("./measure-windows-installed-smoke.ps1", import.meta.url),
    "utf8",
  );
  const signerGate = source.indexOf(
    "$signature.SignerCertificate.Thumbprint.ToLowerInvariant() -eq $ExpectedSignerThumbprint.ToLowerInvariant()",
  );
  const prerequisiteDecision = source.indexOf("$missing = @(");
  const firstInstall = source.indexOf(
    '$payload = Invoke-Install "install-initial"',
  );
  assert.ok(signerGate > 0);
  assert.ok(prerequisiteDecision > signerGate);
  assert.ok(firstInstall > prerequisiteDecision);
});
