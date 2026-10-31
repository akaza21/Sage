import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash, createHmac } from "node:crypto";
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import {
  INSTALLED_SMOKE_STEP_IDS,
  InstalledSmokeOperationError,
  runInstalledNsisSmoke,
  validateInstalledSmokeReceipt,
} from "./beta0001-installed-smoke.mjs";
import {
  INSTALLED_SMOKE_CAMPAIGN_DEADLINE_MS,
  INSTALLED_SMOKE_CAMPAIGN_JOB_TIMEOUT_MS,
  INSTALLED_SMOKE_SETTLEMENT_RESERVE_MS,
  runAuthenticatedInstalledSmoke,
} from "./beta0001-installed-smoke-handoff.mjs";
import { INSTALLED_SMOKE_SETTLEMENT_TIMEOUT_MS } from "./beta0001-installed-smoke-settlement.mjs";
import {
  removeInstalledSmokeTestRoot,
  stageInstalledSmokeTestSource,
} from "./beta0001-installed-smoke-test-source.mjs";

const SHA = "a".repeat(64);
function fileIdentity(filePath) {
  const bytes = readFileSync(filePath);
  return {
    path: filePath,
    sha256: createHash("sha256").update(bytes).digest("hex"),
    bytes: bytes.byteLength,
  };
}

function windowsPowerShell() {
  return path.join(
    process.env.SystemRoot ?? "C:\\Windows",
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
}

function lockedCampaignParameters(options) {
  const controller = JSON.parse(options.stdin.toString("utf8"));
  const campaign = JSON.parse(
    Buffer.from(
      controller.parameters.CampaignBootstrapBase64,
      "base64",
    ).toString("utf8"),
  );
  return { controller: controller.parameters, campaign: campaign.parameters };
}

function completedControllerProof(campaignId, receiptBytes, cleanupKey) {
  return {
    exitCode: 0,
    stdoutBytes: 0,
    stderrBytes: 0,
    stdout: Buffer.from(
      JSON.stringify({
        schemaVersion: 1,
        campaignId,
        status: "completed",
        jobConverged: true,
        childExitCode: 0,
        forced: false,
        orphanProcessesAfterClose: 0,
        receiptSha256: createHash("sha256").update(receiptBytes).digest("hex"),
        receiptAuthenticationSha256: createHmac("sha256", cleanupKey)
          .update(receiptBytes)
          .digest("hex"),
      }),
    ),
  };
}

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

function passingReceipt() {
  return runInstalledNsisSmoke({
    request: request(),
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

test("receipt validation is total for hostile bounded JSON shapes", () => {
  const mutations = [
    (receipt) => {
      receipt.steps = null;
    },
    (receipt) => {
      receipt.steps[0] = null;
    },
    (receipt) => {
      receipt.assertions = null;
    },
    (receipt) => {
      delete receipt.prerequisites;
    },
  ];
  for (const mutate of mutations) {
    const receipt = passingReceipt();
    mutate(receipt);
    let errors;
    assert.doesNotThrow(() => {
      errors = validateInstalledSmokeReceipt(receipt);
    });
    assert.ok(errors.length > 0);
  }
});

test("passing receipts require every prerequisite and the exact observed signer", () => {
  const missingGit = passingReceipt();
  missingGit.prerequisites.git = false;
  assert.ok(
    validateInstalledSmokeReceipt(missingGit).includes(
      "passing receipt has unmet prerequisites",
    ),
  );

  const wrongSigner = passingReceipt();
  wrongSigner.prerequisites.signerThumbprint = "9".repeat(40);
  const errors = validateInstalledSmokeReceipt(wrongSigner);
  assert.ok(errors.includes("receipt signer thumbprint is invalid"));
  assert.ok(errors.includes("passing receipt has unmet prerequisites"));
});

test("prerequisite inspection failures settle as typed receipts", () => {
  let mutations = 0;
  const receipt = runInstalledNsisSmoke({
    request: request(),
    operations: {
      inspectPrerequisites: () => {
        throw new Error("inspection failed");
      },
      runStep: () => {
        mutations += 1;
      },
    },
  });
  assert.equal(mutations, 0);
  assert.equal(receipt.result, "failed");
  assert.deepEqual(receipt.failure, {
    code: "prerequisite-inspection-failed",
    phase: "prerequisites",
  });
  assert.deepEqual(validateInstalledSmokeReceipt(receipt), []);
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

test("failed authoritative uninstall overrides a partial-install failure", () => {
  const receipt = runInstalledNsisSmoke({
    request: request(),
    operations: {
      inspectPrerequisites: () => prerequisites(),
      runStep: (id) => {
        if (id === "install-initial") {
          throw new InstalledSmokeOperationError("step-timeout");
        }
        if (id === "uninstall-final") throw new Error("uninstaller absent");
        throw new Error("unexpected step");
      },
    },
  });
  assert.equal(receipt.result, "failed");
  assert.deepEqual(receipt.failure, {
    code: "cleanup-failed",
    phase: "uninstall-final",
  });
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
    const authenticatedPowerShell = fileIdentity(windowsPowerShell());
    const source = stageInstalledSmokeTestSource(root);
    const handoff = {
      receiptBytes: Buffer.from("package-receipt"),
      receipt: {
        buildIdentity: {
          powershell: {
            executableSha256: authenticatedPowerShell.sha256,
          },
        },
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
        node: fileIdentity(process.execPath),
        git: fileIdentity(process.execPath),
        powershell: authenticatedPowerShell,
      },
      ...source,
    };
    let launch;
    let processCalls = 0;
    const result = await runAuthenticatedInstalledSmoke({
      handoff,
      campaignId: smokeRequest.campaignId,
      outputDirectory: path.join(root, "output"),
      expectedSignerThumbprint: smokeRequest.installer.signerThumbprint,
      runProcess: async (_executable, received, options) => {
        processCalls += 1;
        launch = {
          args: received,
          ...lockedCampaignParameters(options),
        };
        const output = launch.campaign.OutputDirectory;
        receipt.packageReceiptSha256 = "placeholder";
        receipt.packageReceiptSha256 = launch.campaign.PackageReceiptSha256;
        const receiptBytes = Buffer.from(`${JSON.stringify(receipt)}\n`);
        writeFileSync(path.join(output, "receipt.json"), receiptBytes);
        return completedControllerProof(
          smokeRequest.campaignId,
          receiptBytes,
          Buffer.from(launch.campaign.CleanupKeyBase64, "base64"),
        );
      },
    });
    assert.equal(result.result, "passed");
    assert.equal(
      launch.campaign.ExpectedInstalledExecutableSha256,
      smokeRequest.installer.expectedInstalledExecutableSha256,
    );
    assert.equal(
      launch.campaign.ExpectedSignerThumbprint,
      smokeRequest.installer.signerThumbprint,
    );
    assert.equal(launch.campaign.GitSha256, handoff.tools.git.sha256);
    assert.equal(launch.campaign.GitBytes, handoff.tools.git.bytes);
    assert.equal(launch.campaign.NodeSha256, handoff.tools.node.sha256);
    assert.equal(
      launch.campaign.CampaignDeadlineMilliseconds,
      INSTALLED_SMOKE_CAMPAIGN_DEADLINE_MS,
    );
    assert.ok(
      INSTALLED_SMOKE_SETTLEMENT_RESERVE_MS >
        INSTALLED_SMOKE_SETTLEMENT_TIMEOUT_MS,
    );
    assert.equal(
      launch.controller.OuterJobTimeoutMilliseconds,
      INSTALLED_SMOKE_CAMPAIGN_JOB_TIMEOUT_MS,
    );
    assert.equal(launch.args.includes("-File"), false);
    assert.equal(launch.args.includes("-EncodedCommand"), true);
    const replaced = await runAuthenticatedInstalledSmoke({
      handoff,
      campaignId: smokeRequest.campaignId,
      outputDirectory: path.join(root, "replaced-receipt-output"),
      expectedSignerThumbprint: smokeRequest.installer.signerThumbprint,
      runProcess: async (_executable, _received, options) => {
        processCalls += 1;
        const current = lockedCampaignParameters(options);
        const validReceipt = {
          ...receipt,
          packageReceiptSha256: current.campaign.PackageReceiptSha256,
        };
        const validBytes = Buffer.from(`${JSON.stringify(validReceipt)}\n`);
        const proof = completedControllerProof(
          smokeRequest.campaignId,
          validBytes,
          Buffer.from(current.campaign.CleanupKeyBase64, "base64"),
        );
        writeFileSync(
          path.join(current.campaign.OutputDirectory, "receipt.json"),
          `${JSON.stringify({ ...validReceipt, campaignId: "forged-after-exit" })}\n`,
        );
        return proof;
      },
    });
    assert.deepEqual(replaced.failure, {
      code: "receipt-invalid",
      phase: "orchestrator-watchdog",
    });
    assert.equal(replaced.cleanup.status, "absent");
    handoff.artifacts.candidate.installedExecutables.nsis.sha256 = "9".repeat(
      64,
    );
    await assert.rejects(
      runAuthenticatedInstalledSmoke({
        handoff,
        campaignId: smokeRequest.campaignId,
        outputDirectory: path.join(root, "blocked-output"),
        expectedSignerThumbprint: smokeRequest.installer.signerThumbprint,
        runProcess: async () => {
          processCalls += 1;
        },
      }),
      /installed-smoke-authenticated-nsis-binding-invalid/,
    );
    assert.equal(processCalls, 2);
  } finally {
    removeInstalledSmokeTestRoot(root);
  }
});

test("installed smoke rejects runner mutation after artifact handoff authentication", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-runner-mutation-"));
  try {
    const { sourceRoot, sourceManifest } = stageInstalledSmokeTestSource(root);
    writeFileSync(
      path.join(sourceRoot, "scripts", "measure-windows-installed-smoke.ps1"),
      "post-handoff mutation",
    );
    const installerPath = path.join(root, "setup.exe");
    const installedPath = path.join(root, "sage.exe");
    writeFileSync(installerPath, "installer");
    writeFileSync(installedPath, "installed");
    const installer = fileIdentity(installerPath);
    const installed = fileIdentity(installedPath);
    const powershell = fileIdentity(process.execPath);
    let processCalls = 0;
    await assert.rejects(
      runAuthenticatedInstalledSmoke({
        handoff: {
          sourceRoot,
          sourceManifest,
          receiptBytes: Buffer.from("package-receipt"),
          receipt: {
            buildIdentity: {
              powershell: { executableSha256: powershell.sha256 },
            },
            participants: {
              candidate: {
                installers: [{ kind: "nsis", installedExecutable: installed }],
              },
            },
          },
          artifacts: {
            candidate: {
              installerArtifacts: { nsis: installer },
              installedExecutables: { nsis: installed },
            },
          },
          tools: {
            node: fileIdentity(process.execPath),
            git: fileIdentity(process.execPath),
            powershell,
          },
        },
        campaignId: "post-handoff-mutation",
        outputDirectory: path.join(root, "output"),
        expectedSignerThumbprint: "f".repeat(40),
        runProcess: async () => {
          processCalls += 1;
        },
      }),
      /installed-smoke-runner-identity-changed/,
    );
    assert.equal(processCalls, 0);
  } finally {
    removeInstalledSmokeTestRoot(root);
  }
});

test("real PowerShell preflight settles an unsigned candidate without installer mutation", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-installed-preflight-"));
  try {
    const source = stageInstalledSmokeTestSource(root);
    const installerPath = path.join(root, "unsigned-setup.exe");
    const installedPath = path.join(root, "projected-sage.exe");
    writeFileSync(installerPath, "unsigned installer fixture");
    writeFileSync(installedPath, "projected executable fixture");
    const gitPath = execFileSync("where.exe", ["git.exe"], {
      encoding: "utf8",
      windowsHide: true,
    })
      .split(/\r?\n/u)
      .find(Boolean);
    assert.ok(gitPath);
    const installer = fileIdentity(installerPath);
    const installed = fileIdentity(installedPath);
    const powershell = path.join(
      process.env.SystemRoot ?? "C:\\Windows",
      "System32",
      "WindowsPowerShell",
      "v1.0",
      "powershell.exe",
    );
    const handoff = {
      ...source,
      receiptBytes: Buffer.from("preflight-package-receipt"),
      receipt: {
        buildIdentity: {
          powershell: {
            executableSha256: fileIdentity(powershell).sha256,
          },
        },
        participants: {
          candidate: {
            installers: [{ kind: "nsis", installedExecutable: installed }],
          },
        },
      },
      artifacts: {
        candidate: {
          installerArtifacts: { nsis: installer },
          installedExecutables: { nsis: installed },
        },
      },
      tools: {
        node: fileIdentity(process.execPath),
        git: fileIdentity(gitPath),
        powershell: fileIdentity(powershell),
      },
    };
    const receipt = await runAuthenticatedInstalledSmoke({
      handoff,
      campaignId: "unsigned-preflight-01",
      outputDirectory: path.join(root, "output"),
      expectedSignerThumbprint: "f".repeat(40),
    });
    assert.equal(receipt.result, "blocked", JSON.stringify(receipt));
    assert.equal(receipt.failure.phase, "prerequisites");
    assert.equal(
      receipt.prerequisites.authenticode,
      false,
      "unsigned fixture must never reach install",
    );
    assert.equal(
      readFileSync(path.join(root, "output", "receipt.json"), "utf8").length >
        0,
      true,
    );
    assert.equal(existsSync(path.join(root, "output", "installed")), false);
  } finally {
    removeInstalledSmokeTestRoot(root);
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

test("real runner rechecks identities, preserves failure phase, and requires cleanup", () => {
  const source = readFileSync(
    new URL("./measure-windows-installed-smoke.ps1", import.meta.url),
    "utf8",
  );
  const semanticSource = readFileSync(
    new URL("./footprint/InstalledSmokeSemantic.psm1", import.meta.url),
    "utf8",
  );
  const lifecycleSource = readFileSync(
    new URL("./footprint/InstalledSmokePackageLifecycle.psm1", import.meta.url),
    "utf8",
  );
  const signerGate = source.indexOf(
    "$signature.SignerCertificate.Thumbprint.ToLowerInvariant() -eq $ExpectedSignerThumbprint.ToLowerInvariant()",
  );
  const prerequisiteDecision = source.indexOf("$missing = if");
  const firstInstall = source.indexOf(
    '$payload = Invoke-InstalledSmokeInstall $runState $executionGate "install-initial"',
  );
  assert.ok(signerGate > 0);
  assert.ok(prerequisiteDecision > signerGate);
  assert.ok(firstInstall > prerequisiteDecision);
  const installFunction = lifecycleSource.slice(
    lifecycleSource.indexOf("function Invoke-InstalledSmokeInstall"),
    lifecycleSource.indexOf("function Invoke-InstalledSmokeUninstall"),
  );
  assert.ok(
    installFunction.indexOf("Assert-InstalledSmokeInstallerIdentity") <
      installFunction.indexOf(
        'Set-InstalledSmokeCleanupAuthority $Config "installing"',
      ),
  );
  assert.ok(
    installFunction.indexOf(
      'Set-InstalledSmokeCleanupAuthority $Config "installing"',
    ) < installFunction.indexOf("Invoke-InstalledSmokeJobProcess"),
  );
  assert.ok(
    installFunction.indexOf("Get-InstalledSmokeFileIdentity $uninstaller") <
      installFunction.indexOf(
        'Set-InstalledSmokeCleanupAuthority $Config "installed"',
      ),
  );
  const uninstallFunction = lifecycleSource.slice(
    lifecycleSource.indexOf("function Invoke-InstalledSmokeUninstall"),
    lifecycleSource.indexOf("Export-ModuleMember"),
  );
  assert.ok(
    uninstallFunction.indexOf("Assert-InstalledSmokeFileIdentity") <
      uninstallFunction.indexOf("Invoke-InstalledSmokeJobProcess"),
  );
  assert.ok(semanticSource.match(/& \$IdentityGate/gu)?.length >= 2);
  assert.ok(semanticSource.includes("$primaryFailurePhase"));
  assert.ok(source.includes("Assert-CampaignExecutionDeadline"));
  assert.ok(
    source.includes("Partial install has no authoritative uninstaller."),
  );
  assert.ok(source.includes("Assert-RunnerSourceIdentity"));
  assert.ok(
    source.indexOf("Stop-FootprintJobProcess") <
      source.indexOf("if ($runState.installed)"),
  );
});
