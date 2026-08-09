import {
  exactKeys,
  recordDigest,
  safeInteger,
  SHA256,
} from "./ade0006a-c0b-common.mjs";
import {
  deriveCandidateFixtureBinding,
  deriveParticipantBinding,
} from "./ade0006a-package-binding.mjs";
import { validatePackageInstallerEvidence } from "./ade0006a-package-installer-validation.mjs";
import { validatePackageParticipantProvenance } from "./ade0006a-package-provenance-validation.mjs";

const artifactKinds = ["executable", "msi", "nsis"];
const installerKinds = ["msi", "nsis"];

function validateArtifactSet(errors, participant, summary, frozen) {
  const label = `receipt.participants.${participant}.artifactSet`;
  const expected = frozen.package.participants[participant].artifacts;
  exactKeys(
    errors,
    summary?.artifactSet,
    ["entries", "sha256", "bytes"],
    label,
  );
  if (
    !Array.isArray(summary?.artifactSet?.entries) ||
    summary.artifactSet.entries.length !== expected.length
  ) {
    errors.push(`${label} is incomplete`);
    return;
  }
  summary.artifactSet.entries.forEach((entry, index) => {
    const entryLabel = `${label}.entries[${index}]`;
    exactKeys(
      errors,
      entry,
      ["kind", "relativePath", "sha256", "bytes"],
      entryLabel,
    );
    if (
      entry?.kind !== expected[index].kind ||
      entry?.relativePath !== expected[index].relativePath ||
      !SHA256.test(entry?.sha256 ?? "")
    ) {
      errors.push(`${entryLabel} is not contract-bound`);
    }
    safeInteger(
      errors,
      entry?.bytes,
      `${entryLabel}.bytes`,
      1,
      frozen.package.observerBounds[`${expected[index].kind}ArtifactBytes`],
    );
  });
  safeInteger(
    errors,
    summary.artifactSet.bytes,
    `${label}.bytes`,
    1,
    frozen.package.observerBounds.artifactSetBytes,
  );
  if (
    summary.artifactSet.bytes !==
      summary.artifactSet.entries.reduce(
        (total, entry) => total + entry.bytes,
        0,
      ) ||
    summary.artifactSet.sha256 !== recordDigest(summary.artifactSet.entries)
  ) {
    errors.push(`${label} digest or bytes do not reconcile`);
  }
}

function validateBuild(errors, participant, summary, frozen) {
  const label = `receipt.participants.${participant}`;
  exactKeys(
    errors,
    summary?.productionBuild,
    [
      "profile",
      "defaultFeaturesEnabled",
      "enabledFeatures",
      "fingerprintSha256",
    ],
    `${label}.productionBuild`,
  );
  const expected = frozen.package.productionBuild;
  if (
    summary?.productionBuild?.profile !== expected.profile ||
    summary?.productionBuild?.defaultFeaturesEnabled !==
      expected.defaultFeaturesEnabled ||
    JSON.stringify(summary?.productionBuild?.enabledFeatures) !==
      JSON.stringify(expected.enabledFeatures) ||
    !SHA256.test(summary?.productionBuild?.fingerprintSha256 ?? "")
  ) {
    errors.push(`${label}.productionBuild is not observed contract identity`);
  }
}

function validateCharacterization(errors, participant, summary, frozen) {
  if (participant !== frozen.package.characterizationBuild.participant) return;
  const label = `receipt.participants.${participant}.characterization`;
  const value = summary?.characterization;
  exactKeys(
    errors,
    value,
    [
      "executableSha256",
      "executableBytes",
      "featureSet",
      "defaultFeaturesEnabled",
      "fingerprintSha256",
    ],
    label,
  );
  safeInteger(
    errors,
    value?.executableBytes,
    `${label}.executableBytes`,
    1,
    frozen.package.observerBounds.executableArtifactBytes,
  );
  const expected = frozen.package.characterizationBuild;
  if (
    !SHA256.test(value?.executableSha256 ?? "") ||
    !SHA256.test(value?.fingerprintSha256 ?? "") ||
    value?.defaultFeaturesEnabled !== expected.defaultFeaturesEnabled ||
    JSON.stringify(value?.featureSet) !==
      JSON.stringify(expected.enabledFeatures)
  ) {
    errors.push(`${label} is not the frozen characterization build`);
  }
}

function validateFixture(errors, receipt, participant, summary, frozen) {
  if (participant !== frozen.package.fixtureExclusion.participant) return;
  const label = `receipt.participants.${participant}.fixture`;
  const value = summary?.fixture;
  exactKeys(
    errors,
    value,
    ["featureSet", "executableSha256", "executableBytes"],
    label,
  );
  safeInteger(
    errors,
    value?.executableBytes,
    `${label}.executableBytes`,
    1,
    frozen.package.observerBounds.executableArtifactBytes,
  );
  let scopeValid;
  try {
    scopeValid =
      JSON.stringify(Object.keys(deriveCandidateFixtureBinding(receipt))) ===
      JSON.stringify(frozen.package.requiredCandidateFixtureBinding);
  } catch {
    scopeValid = false;
  }
  if (
    !scopeValid ||
    JSON.stringify(value?.featureSet) !==
      JSON.stringify([frozen.package.fixtureExclusion.canonicalFeature]) ||
    !SHA256.test(value?.executableSha256 ?? "")
  ) {
    errors.push(`${label} is invalid or has the wrong binding scope`);
  }
}

function validateBindingScope(errors, receipt, participant, frozen) {
  try {
    const binding = deriveParticipantBinding(receipt, participant);
    const expected = [
      ...frozen.package.requiredSharedBinding,
      ...frozen.package.requiredParticipantBinding,
      ...(participant === frozen.package.characterizationBuild.participant
        ? frozen.package.requiredCandidateCharacterizationBinding
        : []),
    ];
    if (
      JSON.stringify(Object.keys(binding)) !== JSON.stringify(expected) ||
      (participant === frozen.package.characterizationBuild.participant &&
        JSON.stringify(expected) !==
          JSON.stringify(frozen.evidence.requiredCandidateBinding))
    ) {
      errors.push(
        `receipt.participants.${participant} binding scope is invalid`,
      );
    }
  } catch {
    errors.push(`receipt.participants.${participant} binding is incomplete`);
  }
}

export async function validatePackageParticipant({
  errors,
  receipt,
  participant,
  source,
  rawRoot,
  frozen,
}) {
  const summary = receipt?.participants?.[participant];
  const candidate = participant === frozen.package.fixtureExclusion.participant;
  const label = `receipt.participants.${participant}`;
  exactKeys(
    errors,
    summary,
    [
      "participant",
      "configuration",
      "productionBuild",
      "provenance",
      "artifactSet",
      "installers",
      ...(candidate ? ["characterization", "fixture"] : []),
    ],
    label,
  );
  exactKeys(
    errors,
    summary?.configuration,
    [
      "productionTauriBaseConfigSha256",
      "productionTauriWindowsConfigSha256",
      "metadataDeclarationCount",
      ...(candidate ? ["productionFeatureSetContainsFixture"] : []),
    ],
    `${label}.configuration`,
  );
  if (
    summary?.participant !== participant ||
    !SHA256.test(
      summary?.configuration?.productionTauriBaseConfigSha256 ?? "",
    ) ||
    !SHA256.test(
      summary?.configuration?.productionTauriWindowsConfigSha256 ?? "",
    ) ||
    summary?.configuration?.metadataDeclarationCount !==
      frozen.package.fixtureExclusion.metadataDeclarationCount ||
    (candidate &&
      summary?.configuration?.productionFeatureSetContainsFixture !== false)
  ) {
    errors.push(`${label} identity or fixture metadata is invalid`);
  }
  validateArtifactSet(errors, participant, summary, frozen);
  validateBuild(errors, participant, summary, frozen);
  validateCharacterization(errors, participant, summary, frozen);
  validateFixture(errors, receipt, participant, summary, frozen);
  validatePackageParticipantProvenance(
    errors,
    participant,
    summary,
    source,
    frozen,
  );
  validateBindingScope(errors, receipt, participant, frozen);
  if (
    !Array.isArray(summary?.installers) ||
    summary.installers.map((installer) => installer?.kind).join("\0") !==
      installerKinds.join("\0") ||
    summary?.artifactSet?.entries?.map((entry) => entry?.kind).join("\0") !==
      artifactKinds.join("\0")
  ) {
    errors.push(`${label}.installers is incomplete or out of order`);
    return;
  }
  for (const installer of summary.installers) {
    await validatePackageInstallerEvidence({
      errors,
      receipt,
      participant,
      summary,
      installer,
      rawRoot,
      frozen,
    });
  }
}
