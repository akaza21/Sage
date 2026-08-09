import assert from "node:assert/strict";
import {
  cpSync,
  linkSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { loadFrozenContracts } from "./ade0006a-c0b-contracts.mjs";
import { inspectPackageMembers } from "./ade0006a-package-members.mjs";
import {
  derivePackageBudgetEvaluation,
  derivePackageDeltas,
  inspectPackageReceiptSanitized,
  validatePackageReceipt,
} from "./ade0006a-package-receipt.mjs";
import { validateInstallerSidecars } from "./ade0006a-package-sidecar-evidence.mjs";

const frozen = loadFrozenContracts(path.resolve("."));

function campaign() {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-members-"));
  const extracted = path.join(root, "extracted");
  const output = path.join(root, "output");
  mkdirSync(path.join(extracted, "app"), { recursive: true });
  const release = path.join(root, "sage.exe");
  const fixture = path.join(root, "acp-adversarial-fixture.exe");
  writeFileSync(release, "release");
  writeFileSync(fixture, "fixture");
  cpSync(release, path.join(extracted, "sage.exe"));
  writeFileSync(path.join(extracted, "app", "data.bin"), "data");
  return { root, extracted, output, release, fixture };
}

function msiCampaign() {
  const value = campaign();
  rmSync(value.extracted, { recursive: true, force: true });
  mkdirSync(path.join(value.extracted, "File"), { recursive: true });
  mkdirSync(path.join(value.extracted, "Binary"));
  mkdirSync(path.join(value.extracted, "Icon"));
  const stream = path.join(value.extracted, "File", "Path");
  cpSync(value.release, stream);
  writeFileSync(path.join(value.extracted, "Binary", "Dialog"), "metadata");
  writeFileSync(path.join(value.extracted, "Icon", "ProductIcon"), "icon");
  const metadata = path.join(value.root, "source.wxs");
  writeFileSync(
    metadata,
    `<?xml version="1.0" encoding="utf-8"?>
<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi">
  <Directory Id="TARGETDIR" Name="SourceDir">
    <Directory Id="ProgramFiles64Folder" Name="PFiles">
      <Directory Id="INSTALLDIR" Name="Sage">
        <Component Id="Path">
          <File Id="Path" Name="sage.exe" Source="${stream}" />
        </Component>
      </Directory>
    </Directory>
  </Directory>
</Wix>`,
  );
  return { ...value, metadata };
}

async function inspect(value) {
  return inspectPackageMembers({
    participant: "candidate",
    installer: "nsis",
    extractedRoot: value.extracted,
    outputDirectory: value.output,
    fixtureExecutable: value.fixture,
    frozen,
  });
}

test("member inspector writes ordered bounded shards and exact executable evidence", async () => {
  const value = campaign();
  try {
    const result = await inspect(value);
    assert.equal(result.installedExecutable.logicalRelativePath, "sage.exe");
    assert.equal(result.installedExecutable.bytes, 7);
    assert.equal(result.fixtureBasenameCount, 0);
    assert.equal(result.fixtureHashMatchCount, 0);
    assert.equal(result.memberManifest.shardCount, 1);
    assert.equal(result.memberManifest.shards[0].firstRecordOrdinal, 1);
    assert.equal(result.memberManifest.recordCount, 3);
  } finally {
    rmSync(value.root, { recursive: true, force: true });
  }
});

test("member inspector reports fixture content instead of hiding it", async () => {
  const value = campaign();
  try {
    cpSync(
      value.fixture,
      path.join(value.extracted, "acp-adversarial-fixture.exe"),
    );
    const result = await inspect(value);
    assert.equal(result.fixtureBasenameCount, 1);
    assert.equal(result.fixtureHashMatchCount, 1);
  } finally {
    rmSync(value.root, { recursive: true, force: true });
  }
});

test("MSI inspection inventories the logical installed tree, not decompiler metadata", async () => {
  const value = msiCampaign();
  try {
    const result = await inspectPackageMembers({
      participant: "candidate",
      installer: "msi",
      extractedRoot: value.extracted,
      outputDirectory: value.output,
      fixtureExecutable: value.fixture,
      msiMetadataPath: value.metadata,
      frozen,
    });
    assert.equal(
      result.installedExecutable.logicalRelativePath,
      "INSTALLDIR/sage.exe",
    );
    assert.equal(result.installedExecutable.extractedRelativePath, "File/Path");
    assert.equal(result.memberManifest.recordCount, 2);
    assert.equal(result.memberManifest.regularFileCount, 1);
    assert.ok(
      result.memberManifest.extractedRegularFileBytes >
        result.memberManifest.regularFileBytes,
    );
    const sidecar = JSON.parse(
      readFileSync(path.join(value.output, "members-0001.json"), "utf8"),
    );
    assert.deepEqual(
      sidecar.records.map((record) => record.relativePath),
      ["INSTALLDIR", "INSTALLDIR/sage.exe"],
    );
  } finally {
    rmSync(value.root, { recursive: true, force: true });
  }
});

test("MSI inspection rejects an unreferenced cabinet File stream", async () => {
  const value = msiCampaign();
  try {
    writeFileSync(path.join(value.extracted, "File", "Unmapped"), "hidden");
    await assert.rejects(
      inspectPackageMembers({
        participant: "candidate",
        installer: "msi",
        extractedRoot: value.extracted,
        outputDirectory: value.output,
        fixtureExecutable: value.fixture,
        msiMetadataPath: value.metadata,
        frozen,
      }),
      /File streams do not reconcile/,
    );
  } finally {
    rmSync(value.root, { recursive: true, force: true });
  }
});

test("member inspector selects the control executable projection", async () => {
  for (const installer of ["msi", "nsis"]) {
    const value = installer === "msi" ? msiCampaign() : campaign();
    try {
      if (installer === "msi") {
        writeFileSync(
          value.metadata,
          readFileSync(value.metadata, "utf8").replace(
            'Name="sage.exe"',
            'Name="sage-footprint-control.exe"',
          ),
        );
      } else {
        cpSync(
          value.release,
          path.join(value.extracted, "sage-footprint-control.exe"),
        );
        rmSync(path.join(value.extracted, "sage.exe"));
      }
      const result = await inspectPackageMembers({
        participant: "control",
        installer,
        extractedRoot: value.extracted,
        outputDirectory: value.output,
        msiMetadataPath: value.metadata,
        frozen,
      });
      assert.equal(
        result.installedExecutable.logicalRelativePath,
        installer === "msi"
          ? "INSTALLDIR/sage-footprint-control.exe"
          : "sage-footprint-control.exe",
      );
      assert.equal(result.fixtureHashMatchCount, null);
    } finally {
      rmSync(value.root, { recursive: true, force: true });
    }
  }
});

test("receipt validation rehashes the retained member sidecars", async () => {
  const value = campaign();
  const rawRoot = path.join(value.root, "raw");
  const output = path.join(rawRoot, "package", "candidate", "nsis");
  mkdirSync(path.dirname(output), { recursive: true });
  try {
    const summary = await inspectPackageMembers({
      participant: "candidate",
      installer: "nsis",
      extractedRoot: value.extracted,
      outputDirectory: output,
      fixtureExecutable: value.fixture,
      frozen,
    });
    const errors = [];
    await validateInstallerSidecars({
      errors,
      rawRoot,
      participant: "candidate",
      installer: "nsis",
      summary,
      frozen,
    });
    assert.deepEqual(errors, []);
    const sidecarPath = path.join(output, "members-0001.json");
    const sidecar = JSON.parse(readFileSync(sidecarPath, "utf8"));
    sidecar.records[0].relativePath = "changed";
    writeFileSync(sidecarPath, JSON.stringify(sidecar));
    const tamperedErrors = [];
    await validateInstallerSidecars({
      errors: tamperedErrors,
      rawRoot,
      participant: "candidate",
      installer: "nsis",
      summary,
      frozen,
    });
    assert.notDeepEqual(tamperedErrors, []);
  } finally {
    rmSync(value.root, { recursive: true, force: true });
  }
});

test("matched package observations keep runtime control deltas but use the frontend baseline", () => {
  function summary(
    executable,
    msi,
    nsis,
    msiInstalled,
    nsisInstalled,
    frontendRawBytes,
  ) {
    const entries = [
      { kind: "executable", bytes: executable },
      { kind: "msi", bytes: msi },
      { kind: "nsis", bytes: nsis },
    ];
    return {
      artifactSet: {
        entries,
        bytes: entries.reduce((total, entry) => total + entry.bytes, 0),
      },
      provenance: { frontendRawBytes },
      installers: [
        { kind: "msi", memberManifest: { regularFileBytes: msiInstalled } },
        {
          kind: "nsis",
          memberManifest: { regularFileBytes: nsisInstalled },
        },
      ],
    };
  }
  const deltas = derivePackageDeltas(
    summary(6131200, 2854912, 2105694, 6131200, 6131200, 326400),
    summary(3244544, 1597440, 1095102, 3244544, 3244544, 8970),
    { frontendRawBytes: 289190 },
  );
  assert.deepEqual(deltas, {
    frontendRawBytes: 37210,
    productionExecutableBytes: 2886656,
    productionMsiBytes: 1257472,
    productionNsisBytes: 1010592,
    productionArtifactSetBytes: 5154720,
    msiInstalledPayloadBytes: 2886656,
    nsisInstalledPayloadBytes: 2886656,
    installedPayloadSetBytes: 5773312,
  });
  assert.deepEqual(
    derivePackageBudgetEvaluation(deltas, frozen)
      .filter(({ passed }) => !passed)
      .map(({ metric }) => metric),
    [],
  );
});

test("receipt parsing admits the separately bounded source-manifest cardinality", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-receipt-"));
  try {
    for (const participant of ["candidate", "control"]) {
      for (const installer of ["msi", "nsis"]) {
        mkdirSync(path.join(root, "package", participant, installer), {
          recursive: true,
        });
      }
    }
    writeFileSync(
      path.join(root, "source-manifest-v1.json"),
      JSON.stringify({
        aggregateSha256: "0".repeat(64),
        fileCount: 513,
        totalBytes: 513,
        files: Array.from({ length: 513 }, (_, index) => index),
      }),
    );
    const errors = await validatePackageReceipt({
      receipt: {},
      receiptBytes: Buffer.from("{}"),
      rawRoot: root,
      frozen,
    });
    assert.equal(errors.includes("receipt source manifest is invalid"), false);
    assert.ok(errors.length > 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("package receipt sanitization trusts only the frozen-derived budget field", () => {
  const accepted = [];
  inspectPackageReceiptSanitized(
    {
      budgetEvaluation: [
        { metric: "productionExecutableBytes", maximum: 1048576 },
      ],
    },
    accepted,
  );
  assert.deepEqual(accepted, []);
  const rejected = [];
  inspectPackageReceiptSanitized(
    {
      budgetEvaluation: [],
      note: "C:\\sensitive\\path",
    },
    rejected,
  );
  assert.ok(rejected.some((error) => error.includes("absolute path")));
});

test("member inspector shards the exact 512-record boundary", async () => {
  const value = campaign();
  try {
    for (let index = 0; index < 510; index += 1) {
      writeFileSync(
        path.join(value.extracted, `f-${String(index).padStart(4, "0")}`),
        "x",
      );
    }
    const result = await inspect(value);
    assert.equal(result.memberManifest.recordCount, 513);
    assert.equal(result.memberManifest.shardCount, 2);
    assert.equal(result.memberManifest.shards[1].firstRecordOrdinal, 513);
  } finally {
    rmSync(value.root, { recursive: true, force: true });
  }
});

test("member inspector rejects symbolic and hard-linked members", async () => {
  for (const kind of ["symbolic", "hard"]) {
    const value = campaign();
    try {
      const target = path.join(value.extracted, "app", "data.bin");
      const linked = path.join(value.extracted, `${kind}-link.bin`);
      if (kind === "symbolic") symlinkSync(target, linked);
      else linkSync(target, linked);
      await assert.rejects(inspect(value), /linked|symbolic|reparse/);
    } finally {
      rmSync(value.root, { recursive: true, force: true });
    }
  }
});
