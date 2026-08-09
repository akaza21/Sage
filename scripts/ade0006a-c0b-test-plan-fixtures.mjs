import { deriveInstalledExecutableBinding } from "./ade0006a-c0b-package-reference.mjs";

const hash = (character) => character.repeat(64);

function packageParticipant(
  packageReceiptSha256,
  participant,
  installerKind,
  character,
) {
  const value = {
    participant,
    installerKind,
    memberManifestSha256: hash(character),
    installedExecutableSha256: hash(
      String((Number.parseInt(character, 16) + 1) % 10),
    ),
    installedExecutableBytes:
      participant === "candidate" ? 6_000_000 : 3_000_000,
  };
  return {
    ...value,
    executableBindingSha256: deriveInstalledExecutableBinding({
      packageReceiptSha256,
      ...value,
    }),
  };
}

export function makePackageReference(frozen, installerKind = "msi") {
  const receiptSha256 = hash("1");
  return {
    campaignId: "package-pair-fixture",
    receiptSha256,
    contractManifestSha256: frozen.contractManifestSha256,
    packageContractSha256: frozen.packageContractSha256,
    sourceAggregateSha256: hash("2"),
    candidateBindingSha256: hash("c"),
    installerKind,
    participants: {
      candidate: packageParticipant(
        receiptSha256,
        "candidate",
        installerKind,
        "3",
      ),
      control: packageParticipant(receiptSha256, "control", installerKind, "5"),
    },
    fixture: {
      executableSha256: hash("7"),
      executableBytes: 300_000,
    },
    characterization: {
      executableSha256: hash("8"),
      executableBytes: 6_500_000,
      featureSet: ["acp-fixture", "custom-protocol"],
      buildFingerprintSha256: hash("9"),
    },
  };
}

export function makeTrust(frozen, authenticatedPackage) {
  const provenance = frozen.budgets.measurementContract.collectorProvenance;
  return {
    schemaVersion: 1,
    workItem: "ADE-0006A",
    phase: "C0-B",
    packageReceiptSha256: authenticatedPackage.receiptSha256,
    characterization: {
      packageSourceParticipant: "candidate",
      ...authenticatedPackage.characterization,
      fixtureExecutableSha256: authenticatedPackage.fixture.executableSha256,
      fixtureExecutableBytes: authenticatedPackage.fixture.executableBytes,
    },
    environment: {
      testOwner: "independent-tester",
      reviewRole: "independent-test",
      hostClass: "windows-desktop",
      hardware: "x64-host",
      osVersion: "windows-11",
      platform: "windows",
      architecture: "x86_64",
      powerPolicy: "balanced-ac",
      webViewRuntime: "webview2-stable",
    },
    collector: {
      entrypoint: { name: provenance.entrypoint, sha256: hash("a") },
      modules: provenance.modules.map((name, index) => ({
        name,
        sha256: hash(String((index % 5) + 1)),
      })),
      tools: provenance.tools.map((name, index) => ({
        name,
        version: `${index + 1}.0.0`,
        sha256: hash(String(index + 6)),
      })),
    },
  };
}

export function makePlan(trustSha256, frozen, authenticatedPackage) {
  return {
    schemaVersion: 1,
    mode: "acceptance",
    workItem: "ADE-0006A",
    phase: "C0-B",
    trustManifestSha256: trustSha256,
    contractManifestSha256: frozen.contractManifestSha256,
    packagedFootprint: {
      installerKind: authenticatedPackage.installerKind,
    },
    characterization: { participant: "candidate" },
  };
}
