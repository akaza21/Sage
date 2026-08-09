import {
  exactKeys,
  inspectSanitized,
  safeInteger,
  safeSlug,
  SHA256,
} from "./ade0006a-c0b-common.mjs";

function validateCharacterization(errors, value, authenticatedPackage, frozen) {
  exactKeys(
    errors,
    value,
    [
      "packageSourceParticipant",
      "executableSha256",
      "executableBytes",
      "featureSet",
      "buildFingerprintSha256",
      "fixtureExecutableSha256",
      "fixtureExecutableBytes",
    ],
    "trust.characterization",
  );
  if (
    value?.packageSourceParticipant !== "candidate" ||
    !SHA256.test(value?.executableSha256 ?? "") ||
    !SHA256.test(value?.buildFingerprintSha256 ?? "") ||
    JSON.stringify(value?.featureSet) !==
      JSON.stringify(frozen.package.characterizationBuild.enabledFeatures) ||
    value?.executableSha256 !==
      authenticatedPackage.characterization.executableSha256 ||
    value?.executableBytes !==
      authenticatedPackage.characterization.executableBytes ||
    JSON.stringify(value?.featureSet) !==
      JSON.stringify(authenticatedPackage.characterization.featureSet) ||
    value?.buildFingerprintSha256 !==
      authenticatedPackage.characterization.buildFingerprintSha256 ||
    value?.fixtureExecutableSha256 !==
      authenticatedPackage.fixture.executableSha256 ||
    value?.fixtureExecutableBytes !==
      authenticatedPackage.fixture.executableBytes
  ) {
    errors.push("trust.characterization identity is invalid");
  }
  safeInteger(
    errors,
    value?.executableBytes,
    "trust.characterization.executableBytes",
    1,
    frozen.package.observerBounds.executableArtifactBytes,
  );
  safeInteger(
    errors,
    value?.fixtureExecutableBytes,
    "trust.characterization.fixtureExecutableBytes",
    1,
    frozen.package.observerBounds.executableArtifactBytes,
  );
}

function validateEnvironment(errors, value) {
  const keys = [
    "testOwner",
    "reviewRole",
    "hostClass",
    "hardware",
    "osVersion",
    "platform",
    "architecture",
    "powerPolicy",
    "webViewRuntime",
  ];
  exactKeys(errors, value, keys, "trust.environment");
  for (const key of keys)
    safeSlug(errors, value?.[key], `trust.environment.${key}`);
  if (value?.platform !== "windows" || value?.architecture !== "x86_64") {
    errors.push("trust.environment is not an admitted Windows x86_64 host");
  }
}

function validateNamedDigests(errors, values, expectedNames, label) {
  if (
    !Array.isArray(values) ||
    values.length !== expectedNames.length ||
    values.map(({ name }) => name).join("\0") !== expectedNames.join("\0")
  ) {
    errors.push(`${label} is incomplete or out of order`);
    return;
  }
  values.forEach((value, index) => {
    exactKeys(errors, value, ["name", "sha256"], `${label}[${index}]`);
    if (!SHA256.test(value?.sha256 ?? "")) {
      errors.push(`${label}[${index}].sha256 is invalid`);
    }
  });
}

function validateCollector(errors, value, frozen) {
  const contract = frozen.budgets.measurementContract.collectorProvenance;
  exactKeys(
    errors,
    value,
    ["entrypoint", "modules", "tools"],
    "trust.collector",
  );
  exactKeys(
    errors,
    value?.entrypoint,
    ["name", "sha256"],
    "trust.collector.entrypoint",
  );
  if (
    value?.entrypoint?.name !== contract.entrypoint ||
    !SHA256.test(value?.entrypoint?.sha256 ?? "")
  ) {
    errors.push("trust.collector.entrypoint is invalid");
  }
  validateNamedDigests(
    errors,
    value?.modules,
    contract.modules,
    "trust.collector.modules",
  );
  if (
    !Array.isArray(value?.tools) ||
    value.tools.length !== contract.tools.length ||
    value.tools.map(({ name }) => name).join("\0") !== contract.tools.join("\0")
  ) {
    errors.push("trust.collector.tools is incomplete or out of order");
    return;
  }
  value.tools.forEach((tool, index) => {
    const label = `trust.collector.tools[${index}]`;
    exactKeys(errors, tool, ["name", "version", "sha256"], label);
    if (
      !/^[a-z0-9][a-z0-9.+_-]{0,63}$/i.test(tool?.version ?? "") ||
      !SHA256.test(tool?.sha256 ?? "")
    ) {
      errors.push(`${label} identity is invalid`);
    }
  });
}

export function validateTrustManifest(
  trust,
  trustSha256,
  expectedTrustSha256,
  frozen,
  authenticatedPackage,
) {
  const errors = [];
  exactKeys(
    errors,
    trust,
    [
      "schemaVersion",
      "workItem",
      "phase",
      "packageReceiptSha256",
      "characterization",
      "environment",
      "collector",
    ],
    "trust",
  );
  if (
    trust?.schemaVersion !== 1 ||
    trust?.workItem !== "ADE-0006A" ||
    trust?.phase !== "C0-B" ||
    !SHA256.test(trustSha256 ?? "") ||
    trustSha256 !== expectedTrustSha256 ||
    trust?.packageReceiptSha256 !== authenticatedPackage.receiptSha256
  ) {
    errors.push("trust manifest header or package receipt binding is invalid");
  }
  validateCharacterization(
    errors,
    trust?.characterization,
    authenticatedPackage,
    frozen,
  );
  validateEnvironment(errors, trust?.environment);
  validateCollector(errors, trust?.collector, frozen);
  inspectSanitized(trust, errors, "trust", ["environment"]);
  return errors;
}
