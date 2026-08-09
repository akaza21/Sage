function featureSection(source) {
  return source.match(/(?:^|\n)\[features\]\r?\n([\s\S]*?)(?=\r?\n\[|$)/)?.[1];
}

export function cargoPackageFeatureErrors(
  source,
  packageContract,
  participant = "candidate",
) {
  const errors = [];
  const features = featureSection(source);
  if (!features) return ["Cargo.toml must contain a [features] section."];
  if (!/^default\s*=\s*\[\s*\]\s*$/m.test(features)) {
    errors.push(
      "Cargo default features must remain empty for production-default.",
    );
  }
  if (
    !/^custom-protocol\s*=\s*\[\s*"tauri\/custom-protocol"\s*\]\s*$/m.test(
      features,
    )
  ) {
    errors.push(
      "Cargo custom-protocol feature must remain explicit and exact.",
    );
  }
  const fixture = packageContract.fixtureExclusion.canonicalFeature;
  const escaped = fixture.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  if (
    participant === packageContract.fixtureExclusion.participant &&
    !new RegExp(`^${escaped}\\s*=\\s*\\[\\s*\\]\\s*$`, "m").test(features)
  ) {
    errors.push(
      `Cargo fixture feature ${fixture} must remain explicit and empty.`,
    );
  }
  return errors;
}
