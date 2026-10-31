import assert from "node:assert/strict";
import { createHash, createHmac } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { runAuthenticatedInstalledSmoke } from "./beta0001-installed-smoke-handoff.mjs";
import { settleInstalledSmokeWatchdog } from "./beta0001-installed-smoke-settlement.mjs";
import {
  acquireInstalledSmokeOutputAuthority,
  INSTALLED_SMOKE_OUTPUT_GUARDIAN_ARGUMENTS,
  releaseInstalledSmokeOutputAuthority,
} from "./beta0001-installed-smoke-output-authority.mjs";
import {
  removeInstalledSmokeTestRoot,
  stageInstalledSmokeTestSource,
} from "./beta0001-installed-smoke-test-source.mjs";

function identity(filePath) {
  const bytes = readFileSync(filePath);
  return {
    path: filePath,
    sha256: createHash("sha256").update(bytes).digest("hex"),
    bytes: bytes.byteLength,
  };
}

test("output guardian revalidates each ancestor after pinning its no-delete handle", () => {
  const encodedIndex =
    INSTALLED_SMOKE_OUTPUT_GUARDIAN_ARGUMENTS.indexOf("-EncodedCommand");
  assert.notEqual(encodedIndex, -1);
  const source = Buffer.from(
    INSTALLED_SMOKE_OUTPUT_GUARDIAN_ARGUMENTS[encodedIndex + 1],
    "base64",
  ).toString("utf16le");
  const loop = source.match(
    /foreach \(string ancestor in ancestors\) \{([\s\S]*?)\n\s*\}/u,
  )?.[1];
  assert.ok(loop, "ancestor sealing loop must remain explicit and reviewable");
  assert.match(
    loop,
    /RequirePlainDirectory\(ancestor\);\s*var handle = OpenDirectory\(ancestor\);\s*seal\.handles\.Add\(handle\);\s*RequirePlainDirectory\(ancestor\);/u,
  );
});

function campaignBootstrap(options) {
  const controller = JSON.parse(options.stdin.toString("utf8"));
  return JSON.parse(
    Buffer.from(
      controller.parameters.CampaignBootstrapBase64,
      "base64",
    ).toString("utf8"),
  );
}

function controllerProof(
  campaignId,
  status = "timeout",
  receiptBytes = null,
  cleanupKey = null,
) {
  return {
    exitCode: 0,
    stdoutBytes: 0,
    stderrBytes: 0,
    stdout: Buffer.from(
      JSON.stringify({
        schemaVersion: 1,
        campaignId,
        status,
        jobConverged: true,
        childExitCode: status === "completed" ? 0 : null,
        forced: status !== "completed",
        orphanProcessesAfterClose: 0,
        receiptSha256: receiptBytes
          ? createHash("sha256").update(receiptBytes).digest("hex")
          : null,
        receiptAuthenticationSha256:
          receiptBytes && cleanupKey
            ? createHmac("sha256", cleanupKey)
                .update(receiptBytes)
                .digest("hex")
            : null,
      }),
    ),
  };
}

function smokeHandoff(root) {
  const installerPath = path.join(root, "setup.exe");
  const installedProjection = path.join(root, "sage.exe");
  writeFileSync(installerPath, "safe unsigned setup fixture");
  writeFileSync(installedProjection, "safe installed projection");
  const installer = identity(installerPath);
  const installed = identity(installedProjection);
  const powershellPath = path.join(
    process.env.SystemRoot ?? "C:\\Windows",
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const powershell = identity(powershellPath);
  const source = stageInstalledSmokeTestSource(root);
  return {
    ...source,
    receiptBytes: Buffer.from("watchdog-package-receipt"),
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
      node: identity(process.execPath),
      git: identity(process.execPath),
      powershell,
    },
  };
}

async function settleExistingOutput(options) {
  const outputAuthority = await acquireInstalledSmokeOutputAuthority({
    outputDirectory: options.outputDirectory,
    powerShell: options.powershell,
  });
  try {
    return await settleInstalledSmokeWatchdog({
      ...options,
      outputAuthority,
    });
  } finally {
    await releaseInstalledSmokeOutputAuthority(outputAuthority);
  }
}

const testCleanupKey = Buffer.alloc(32, 0x5a);

function cleanupAuthority(campaignId, uninstaller, key = testCleanupKey) {
  const record = {
    schemaVersion: 1,
    campaignId,
    state: "installed",
    installRootLabel: "installed",
    uninstaller: {
      relativePath: "installed/uninstall.exe",
      sha256: uninstaller.sha256,
      bytes: uninstaller.bytes,
    },
  };
  const canonical = [
    "schemaVersion=1",
    `campaignId=${campaignId}`,
    "state=installed",
    "installRootLabel=installed",
    "relativePath=installed/uninstall.exe",
    `sha256=${uninstaller.sha256}`,
    `bytes=${uninstaller.bytes}`,
    "",
  ].join("\n");
  return {
    ...record,
    authenticationSha256: createHmac("sha256", key)
      .update(canonical)
      .digest("hex"),
  };
}

test("forced watchdog performs exact Job-contained fallback cleanup", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-watchdog-cleanup-"));
  try {
    const output = path.join(root, "output");
    const installed = path.join(output, "installed");
    mkdirSync(installed, { recursive: true });
    const uninstallerPath = path.join(installed, "uninstall.exe");
    copyFileSync(
      path.join(
        process.env.SystemRoot ?? "C:\\Windows",
        "System32",
        "attrib.exe",
      ),
      uninstallerPath,
    );
    const uninstaller = identity(uninstallerPath);
    writeFileSync(
      path.join(output, "cleanup-authority.json"),
      `${JSON.stringify(
        cleanupAuthority("watchdog-cleanup-01", uninstaller),
      )}\n`,
    );
    const powershellPath = path.join(
      process.env.SystemRoot ?? "C:\\Windows",
      "System32",
      "WindowsPowerShell",
      "v1.0",
      "powershell.exe",
    );
    const result = await settleExistingOutput({
      campaignId: "watchdog-cleanup-01",
      outputDirectory: output,
      ...stageInstalledSmokeTestSource(root),
      powershell: identity(powershellPath),
      cleanupKey: testCleanupKey,
      outerJobConverged: true,
    });
    assert.deepEqual(result, {
      status: "uninstalled",
      authoritySha256: result.authoritySha256,
      residueRelativePath: null,
      outerJobConverged: true,
      fallbackJobConverged: true,
    });
    assert.match(result.authoritySha256, /^[0-9a-f]{64}$/u);
    assert.equal(existsSync(installed), false);
    assert.equal(
      existsSync(path.join(output, "cleanup-authority.json")),
      false,
    );
  } finally {
    removeInstalledSmokeTestRoot(root);
  }
});

test("forced outer watchdog returns truthful failure after fallback uninstall", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-watchdog-orchestrator-"));
  try {
    const installerPath = path.join(root, "setup.exe");
    const installedProjection = path.join(root, "sage.exe");
    writeFileSync(installerPath, "safe unsigned setup fixture");
    writeFileSync(installedProjection, "safe installed projection");
    const installer = identity(installerPath);
    const installed = identity(installedProjection);
    const powershellPath = path.join(
      process.env.SystemRoot ?? "C:\\Windows",
      "System32",
      "WindowsPowerShell",
      "v1.0",
      "powershell.exe",
    );
    const powershell = identity(powershellPath);
    const source = stageInstalledSmokeTestSource(root);
    const campaignId = "watchdog-orchestrator-01";
    const outputDirectory = path.join(root, "output");
    const settlement = await runAuthenticatedInstalledSmoke({
      handoff: {
        ...source,
        receiptBytes: Buffer.from("watchdog-package-receipt"),
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
          node: identity(process.execPath),
          git: identity(process.execPath),
          powershell,
        },
      },
      campaignId,
      outputDirectory,
      expectedSignerThumbprint: "f".repeat(40),
      runProcess: async (_executable, _args, options) => {
        const bootstrap = campaignBootstrap(options);
        const output = bootstrap.parameters.OutputDirectory;
        const installRoot = path.join(output, "installed");
        mkdirSync(installRoot);
        const uninstallerPath = path.join(installRoot, "uninstall.exe");
        copyFileSync(
          path.join(
            process.env.SystemRoot ?? "C:\\Windows",
            "System32",
            "attrib.exe",
          ),
          uninstallerPath,
        );
        const uninstaller = identity(uninstallerPath);
        const cleanupKey = Buffer.from(
          bootstrap.parameters.CleanupKeyBase64,
          "base64",
        );
        writeFileSync(
          path.join(output, "cleanup-authority.json"),
          `${JSON.stringify(
            cleanupAuthority(campaignId, uninstaller, cleanupKey),
          )}\n`,
        );
        return controllerProof(campaignId);
      },
    });
    assert.equal(settlement.result, "failed");
    assert.deepEqual(settlement.failure, {
      code: "orchestrator-timeout",
      phase: "orchestrator-watchdog",
    });
    assert.equal(settlement.cleanup.status, "uninstalled");
    assert.equal(settlement.cleanup.outerJobConverged, true);
    assert.equal(settlement.cleanup.fallbackJobConverged, true);
    assert.equal(existsSync(path.join(outputDirectory, "installed")), false);
    assert.equal(
      JSON.parse(
        readFileSync(
          path.join(outputDirectory, "orchestrator-failure.json"),
          "utf8",
        ),
      ).failure.code,
      "orchestrator-timeout",
    );
  } finally {
    removeInstalledSmokeTestRoot(root);
  }
});

test("outer watchdog without zero-membership proof retains residue and never launches fallback", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-watchdog-no-proof-"));
  try {
    const campaignId = "watchdog-no-proof-01";
    const outputDirectory = path.join(root, "output");
    let fallbackCalls = 0;
    const settlement = await runAuthenticatedInstalledSmoke({
      handoff: smokeHandoff(root),
      campaignId,
      outputDirectory,
      expectedSignerThumbprint: "f".repeat(40),
      runProcess: async (_executable, _args, options) => {
        const bootstrap = campaignBootstrap(options);
        const installRoot = path.join(
          bootstrap.parameters.OutputDirectory,
          "installed",
        );
        mkdirSync(installRoot);
        const uninstallerPath = path.join(installRoot, "uninstall.exe");
        copyFileSync(
          path.join(
            process.env.SystemRoot ?? "C:\\Windows",
            "System32",
            "attrib.exe",
          ),
          uninstallerPath,
        );
        const cleanupKey = Buffer.from(
          bootstrap.parameters.CleanupKeyBase64,
          "base64",
        );
        writeFileSync(
          path.join(outputDirectory, "cleanup-authority.json"),
          `${JSON.stringify(
            cleanupAuthority(campaignId, identity(uninstallerPath), cleanupKey),
          )}\n`,
        );
        throw new Error("installed-smoke-process-timeout");
      },
      runFallbackProcess: async () => {
        fallbackCalls += 1;
      },
    });
    assert.equal(fallbackCalls, 0);
    assert.deepEqual(settlement.failure, {
      code: "cleanup-failed",
      phase: "fallback-cleanup",
    });
    assert.equal(settlement.cleanup.status, "retained");
    assert.equal(settlement.cleanup.outerJobConverged, false);
    assert.equal(settlement.cleanup.fallbackJobConverged, false);
    assert.equal(
      existsSync(path.join(outputDirectory, "installed", "uninstall.exe")),
      true,
    );
  } finally {
    removeInstalledSmokeTestRoot(root);
  }
});

test("wrapper-held output authority blocks post-validation root substitution", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-watchdog-root-seal-"));
  try {
    const campaignId = "watchdog-root-seal-01";
    const outputDirectory = path.join(root, "output");
    const moved = path.join(root, "moved-output");
    const settlement = await runAuthenticatedInstalledSmoke({
      handoff: smokeHandoff(root),
      campaignId,
      outputDirectory,
      expectedSignerThumbprint: "f".repeat(40),
      runProcess: async () => {
        assert.throws(
          () => renameSync(outputDirectory, moved),
          (error) => ["EBUSY", "EPERM", "EACCES"].includes(error?.code),
        );
        return controllerProof(campaignId);
      },
    });
    assert.equal(
      settlement.failure.code,
      "orchestrator-timeout",
      JSON.stringify(settlement),
    );
    assert.equal(settlement.cleanup.status, "absent");
    assert.equal(existsSync(outputDirectory), true);
    assert.equal(existsSync(moved), false);
    assert.equal(
      existsSync(path.join(outputDirectory, "orchestrator-failure.json")),
      true,
    );
  } finally {
    removeInstalledSmokeTestRoot(root);
  }
});

test("zero-exit campaigns with missing or malformed receipts settle residue fail-closed", async (t) => {
  for (const receiptCase of ["missing", "malformed", "mismatched"]) {
    await t.test(receiptCase, async () => {
      const root = mkdtempSync(
        path.join(tmpdir(), `sage-watchdog-receipt-${receiptCase}-`),
      );
      try {
        const campaignId = `watchdog-receipt-${receiptCase}`;
        const outputDirectory = path.join(root, "output");
        const settlement = await runAuthenticatedInstalledSmoke({
          handoff: smokeHandoff(root),
          campaignId,
          outputDirectory,
          expectedSignerThumbprint: "f".repeat(40),
          runProcess: async (_executable, _args, options) => {
            const bootstrap = campaignBootstrap(options);
            const installRoot = path.join(
              bootstrap.parameters.OutputDirectory,
              "installed",
            );
            mkdirSync(installRoot);
            const uninstallerPath = path.join(installRoot, "uninstall.exe");
            copyFileSync(
              path.join(
                process.env.SystemRoot ?? "C:\\Windows",
                "System32",
                "attrib.exe",
              ),
              uninstallerPath,
            );
            const cleanupKey = Buffer.from(
              bootstrap.parameters.CleanupKeyBase64,
              "base64",
            );
            writeFileSync(
              path.join(outputDirectory, "cleanup-authority.json"),
              `${JSON.stringify(
                cleanupAuthority(
                  campaignId,
                  identity(uninstallerPath),
                  cleanupKey,
                ),
              )}\n`,
            );
            let receiptBytes = null;
            if (receiptCase === "malformed") {
              receiptBytes = Buffer.from("{");
            } else if (receiptCase === "mismatched") {
              receiptBytes = Buffer.from(
                '{"schemaVersion":1,"campaignId":"stale-campaign"}\n',
              );
            }
            if (receiptBytes) {
              writeFileSync(
                path.join(outputDirectory, "receipt.json"),
                receiptBytes,
              );
            }
            return controllerProof(
              campaignId,
              "completed",
              receiptBytes,
              cleanupKey,
            );
          },
        });
        assert.deepEqual(settlement.failure, {
          code: "receipt-invalid",
          phase: "orchestrator-watchdog",
        });
        assert.equal(settlement.cleanup.status, "uninstalled");
        assert.equal(settlement.cleanup.outerJobConverged, true);
        assert.equal(settlement.cleanup.fallbackJobConverged, true);
        assert.equal(
          existsSync(path.join(outputDirectory, "installed")),
          false,
        );
      } finally {
        removeInstalledSmokeTestRoot(root);
      }
    });
  }
});

test("forced watchdog retains residue before a changed uninstaller can run", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-watchdog-tamper-"));
  try {
    const output = path.join(root, "output");
    const installed = path.join(output, "installed");
    mkdirSync(installed, { recursive: true });
    const uninstallerPath = path.join(installed, "uninstall.exe");
    copyFileSync(
      path.join(
        process.env.SystemRoot ?? "C:\\Windows",
        "System32",
        "attrib.exe",
      ),
      uninstallerPath,
    );
    const uninstaller = identity(uninstallerPath);
    writeFileSync(path.join(installed, "sentinel.txt"), "must remain");
    writeFileSync(
      path.join(output, "cleanup-authority.json"),
      `${JSON.stringify(
        cleanupAuthority("watchdog-tamper-01", uninstaller),
      )}\n`,
    );
    writeFileSync(uninstallerPath, "changed after authority capture");
    const powershellPath = path.join(
      process.env.SystemRoot ?? "C:\\Windows",
      "System32",
      "WindowsPowerShell",
      "v1.0",
      "powershell.exe",
    );
    const result = await settleExistingOutput({
      campaignId: "watchdog-tamper-01",
      outputDirectory: output,
      ...stageInstalledSmokeTestSource(root),
      powershell: identity(powershellPath),
      cleanupKey: testCleanupKey,
      outerJobConverged: true,
    });
    assert.equal(result.status, "retained");
    assert.equal(result.residueRelativePath, "installed");
    assert.equal(result.fallbackJobConverged, false);
    assert.equal(
      readFileSync(path.join(output, "installed", "sentinel.txt"), "utf8"),
      "must remain",
    );
    assert.equal(existsSync(path.join(output, "fallback-cleanup.json")), false);
  } finally {
    removeInstalledSmokeTestRoot(root);
  }
});

test("forced watchdog rejects a forged durable cleanup authority", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-watchdog-forgery-"));
  try {
    const output = path.join(root, "output");
    const installed = path.join(output, "installed");
    mkdirSync(installed, { recursive: true });
    const uninstallerPath = path.join(installed, "uninstall.exe");
    copyFileSync(
      path.join(
        process.env.SystemRoot ?? "C:\\Windows",
        "System32",
        "attrib.exe",
      ),
      uninstallerPath,
    );
    const forged = cleanupAuthority(
      "watchdog-forgery-01",
      identity(uninstallerPath),
    );
    forged.authenticationSha256 = "0".repeat(64);
    writeFileSync(
      path.join(output, "cleanup-authority.json"),
      `${JSON.stringify(forged)}\n`,
    );
    const powershellPath = path.join(
      process.env.SystemRoot ?? "C:\\Windows",
      "System32",
      "WindowsPowerShell",
      "v1.0",
      "powershell.exe",
    );
    const result = await settleExistingOutput({
      campaignId: "watchdog-forgery-01",
      outputDirectory: output,
      ...stageInstalledSmokeTestSource(root),
      powershell: identity(powershellPath),
      cleanupKey: testCleanupKey,
      outerJobConverged: true,
    });
    assert.equal(result.status, "retained");
    assert.equal(result.fallbackJobConverged, false);
    assert.equal(
      existsSync(path.join(output, "installed", "uninstall.exe")),
      true,
    );
    assert.equal(existsSync(path.join(output, "fallback-cleanup.json")), false);
  } finally {
    removeInstalledSmokeTestRoot(root);
  }
});

test("watchdog never reports cleanup absent while durable authority remains", async () => {
  const root = mkdtempSync(
    path.join(tmpdir(), "sage-watchdog-authority-only-"),
  );
  try {
    const output = path.join(root, "output");
    mkdirSync(output);
    writeFileSync(path.join(output, "cleanup-authority.json"), "retained\n");
    const powershell = identity(
      path.join(
        process.env.SystemRoot ?? "C:\\Windows",
        "System32",
        "WindowsPowerShell",
        "v1.0",
        "powershell.exe",
      ),
    );
    const result = await settleExistingOutput({
      campaignId: "watchdog-authority-only-01",
      outputDirectory: output,
      ...stageInstalledSmokeTestSource(root),
      powershell,
      cleanupKey: testCleanupKey,
      outerJobConverged: false,
    });
    assert.equal(result.status, "retained");
    assert.equal(result.residueRelativePath, "cleanup-authority.json");
  } finally {
    removeInstalledSmokeTestRoot(root);
  }
});

test("converged watchdog treats an installed junction as unresolved without mutating its target", async () => {
  const root = mkdtempSync(
    path.join(tmpdir(), "sage-watchdog-installed-link-"),
  );
  try {
    const output = path.join(root, "output");
    const target = path.join(root, "outside");
    mkdirSync(output);
    mkdirSync(target);
    writeFileSync(path.join(target, "sentinel.txt"), "must remain");
    symlinkSync(target, path.join(output, "installed"), "junction");
    const powershell = identity(
      path.join(
        process.env.SystemRoot ?? "C:\\Windows",
        "System32",
        "WindowsPowerShell",
        "v1.0",
        "powershell.exe",
      ),
    );
    const result = await settleExistingOutput({
      campaignId: "watchdog-installed-link-01",
      outputDirectory: output,
      ...stageInstalledSmokeTestSource(root),
      powershell,
      cleanupKey: testCleanupKey,
      outerJobConverged: true,
    });
    assert.equal(result.status, "unresolved");
    assert.equal(result.residueRelativePath, null);
    assert.equal(result.outerJobConverged, true);
    assert.equal(
      readFileSync(path.join(target, "sentinel.txt"), "utf8"),
      "must remain",
    );
    assert.equal(existsSync(path.join(output, "quarantine-installed")), false);
  } finally {
    removeInstalledSmokeTestRoot(root);
  }
});

test("settlement never mutates a junction target after output-root validation fails", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-watchdog-junction-"));
  try {
    const target = path.join(root, "outside");
    mkdirSync(target, { recursive: true });
    writeFileSync(path.join(target, "sentinel.txt"), "must remain");
    const junction = path.join(root, "output-junction");
    symlinkSync(target, junction, "junction");
    const powershellPath = path.join(
      process.env.SystemRoot ?? "C:\\Windows",
      "System32",
      "WindowsPowerShell",
      "v1.0",
      "powershell.exe",
    );
    await assert.rejects(
      acquireInstalledSmokeOutputAuthority({
        outputDirectory: path.join(junction, "new-output"),
        powerShell: identity(powershellPath),
        create: true,
      }),
      /installed-smoke-output-authority-exit-/,
    );
    assert.equal(
      readFileSync(path.join(target, "sentinel.txt"), "utf8"),
      "must remain",
    );
    assert.equal(existsSync(path.join(target, "new-output")), false);
  } finally {
    removeInstalledSmokeTestRoot(root);
  }
});

test("forced outer watchdog reports cleanup-failed with retained residue", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-watchdog-failure-"));
  try {
    const installerPath = path.join(root, "setup.exe");
    const installedProjection = path.join(root, "sage.exe");
    writeFileSync(installerPath, "safe unsigned setup fixture");
    writeFileSync(installedProjection, "safe installed projection");
    const installer = identity(installerPath);
    const installed = identity(installedProjection);
    const powershellPath = path.join(
      process.env.SystemRoot ?? "C:\\Windows",
      "System32",
      "WindowsPowerShell",
      "v1.0",
      "powershell.exe",
    );
    const powershell = identity(powershellPath);
    const source = stageInstalledSmokeTestSource(root);
    const campaignId = "watchdog-failure-01";
    const outputDirectory = path.join(root, "output");
    const settlement = await runAuthenticatedInstalledSmoke({
      handoff: {
        ...source,
        receiptBytes: Buffer.from("watchdog-package-receipt"),
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
          node: identity(process.execPath),
          git: identity(process.execPath),
          powershell,
        },
      },
      campaignId,
      outputDirectory,
      expectedSignerThumbprint: "f".repeat(40),
      runProcess: async (_executable, _args, options) => {
        const bootstrap = campaignBootstrap(options);
        const output = bootstrap.parameters.OutputDirectory;
        const installRoot = path.join(output, "installed");
        mkdirSync(installRoot);
        const uninstallerPath = path.join(installRoot, "uninstall.exe");
        copyFileSync(
          path.join(
            process.env.SystemRoot ?? "C:\\Windows",
            "System32",
            "attrib.exe",
          ),
          uninstallerPath,
        );
        const uninstaller = identity(uninstallerPath);
        const cleanupKey = Buffer.from(
          bootstrap.parameters.CleanupKeyBase64,
          "base64",
        );
        writeFileSync(path.join(installRoot, "sentinel.txt"), "retained");
        writeFileSync(
          path.join(output, "cleanup-authority.json"),
          `${JSON.stringify(
            cleanupAuthority(campaignId, uninstaller, cleanupKey),
          )}\n`,
        );
        writeFileSync(uninstallerPath, "changed after authority capture");
        return controllerProof(campaignId);
      },
    });
    assert.deepEqual(settlement.failure, {
      code: "cleanup-failed",
      phase: "fallback-cleanup",
    });
    assert.equal(settlement.cleanup.status, "retained");
    assert.equal(settlement.cleanup.residueRelativePath, "installed");
    assert.equal(
      readFileSync(
        path.join(outputDirectory, "installed", "sentinel.txt"),
        "utf8",
      ),
      "retained",
    );
  } finally {
    removeInstalledSmokeTestRoot(root);
  }
});
