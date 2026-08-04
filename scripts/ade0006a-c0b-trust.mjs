import {
  exactKeys,
  HEX_ID,
  inspectSanitized,
  safeInteger,
  safeSlug,
  SHA256,
} from "./ade0006a-c0b-common.mjs";

const bindingKeys = [
  "kind",
  "sourceCommit",
  "sourceTree",
  "sourceAggregateSha256",
  "sourceManifestSha256",
  "sourceFileCount",
  "sourceBytes",
  "contractManifestSha256",
  "cargoLockSha256",
  "frontendLockSha256",
  "executableSha256",
  "executableBytes",
  "featureSet",
  "platform",
  "architecture",
  "dependencyGraphSha256",
  "dependencyPackageCount",
  "frontendRawBytes",
  "productionPackageSha256",
  "productionPackageBytes",
  "installerSha256",
  "installerBytes",
  "installedCoreBytes",
];

function validateBinding(errors, binding, kind) {
  exactKeys(errors, binding, bindingKeys, `trust.${kind}`);
  if (binding?.kind !== kind) errors.push(`trust.${kind}.kind is wrong`);
  if (!/^[0-9a-f]{40}$/.test(binding?.sourceCommit ?? "")) {
    errors.push(`trust.${kind}.sourceCommit is invalid`);
  }
  if (!HEX_ID.test(binding?.sourceTree ?? "")) {
    errors.push(`trust.${kind}.sourceTree is invalid`);
  }
  for (const key of bindingKeys.filter((key) => key.endsWith("Sha256"))) {
    if (!SHA256.test(binding?.[key] ?? "")) {
      errors.push(`trust.${kind}.${key} is invalid`);
    }
  }
  for (const key of [
    "sourceFileCount",
    "sourceBytes",
    "executableBytes",
    "dependencyPackageCount",
    "frontendRawBytes",
    "productionPackageBytes",
    "installerBytes",
    "installedCoreBytes",
  ]) {
    safeInteger(errors, binding?.[key], `trust.${kind}.${key}`, 0, 2 ** 40);
  }
  if (
    !Array.isArray(binding?.featureSet) ||
    binding.featureSet.length < 1 ||
    binding.featureSet.length > 32 ||
    binding.featureSet.some((entry) => !/^[a-z0-9][a-z0-9-]{0,63}$/.test(entry))
  ) {
    errors.push(`trust.${kind}.featureSet is invalid`);
  }
  safeSlug(errors, binding?.platform, `trust.${kind}.platform`);
  safeSlug(errors, binding?.architecture, `trust.${kind}.architecture`);
}

export function validateTrustManifest(
  trust,
  trustSha256,
  expectedSha256,
  frozen,
) {
  const errors = [];
  exactKeys(
    errors,
    trust,
    [
      "schemaVersion",
      "workItem",
      "phase",
      "candidate",
      "control",
      "provenance",
      "tools",
    ],
    "trust",
  );
  if (trustSha256 !== expectedSha256 || !SHA256.test(expectedSha256 ?? "")) {
    errors.push("trust manifest does not match the independent expected hash");
  }
  if (
    trust?.schemaVersion !== 1 ||
    trust?.workItem !== "ADE-0006A" ||
    trust?.phase !== "C0-B"
  ) {
    errors.push("trust manifest header is invalid");
  }
  validateBinding(errors, trust?.candidate, "candidate");
  validateBinding(errors, trust?.control, "control");
  if (
    trust?.candidate?.contractManifestSha256 !== frozen.contractManifestSha256
  ) {
    errors.push("candidate is not bound to the frozen contract manifest");
  }
  if (trust?.candidate?.executableSha256 === trust?.control?.executableSha256) {
    errors.push("candidate and control executables must be distinct");
  }
  if (
    trust?.candidate?.platform !== trust?.control?.platform ||
    trust?.candidate?.architecture !== trust?.control?.architecture
  ) {
    errors.push("candidate and control platform/architecture are not matched");
  }
  if (
    !Array.isArray(trust?.tools) ||
    trust.tools.length < 1 ||
    trust.tools.length > 32
  ) {
    errors.push("trust.tools must contain 1..32 authenticated tools");
  } else {
    const names = new Set();
    trust.tools.forEach((tool, index) => {
      exactKeys(errors, tool, ["name", "sha256"], `trust.tools[${index}]`);
      safeSlug(errors, tool?.name, `trust.tools[${index}].name`);
      if (!SHA256.test(tool?.sha256 ?? "")) {
        errors.push(`trust.tools[${index}].sha256 is invalid`);
      }
      if (names.has(tool?.name)) {
        errors.push("trust.tools contains a duplicate name");
      }
      names.add(tool?.name);
    });
  }
  inspectSanitized(trust, errors, "trust");
  return errors;
}
