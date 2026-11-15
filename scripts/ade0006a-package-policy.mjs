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
  // The feature may only turn on the `fixture` feature of Sage's own crates
  // (spec 7.5), never an external dependency.
  const forward = '"sage-[a-z0-9-]+/fixture"';
  if (
    participant === packageContract.fixtureExclusion.participant &&
    !new RegExp(
      `^${escaped}\\s*=\\s*\\[\\s*(?:${forward}(?:\\s*,\\s*${forward})*\\s*,?\\s*)?\\]\\s*$`,
      "m",
    ).test(features)
  ) {
    errors.push(
      `Cargo fixture feature ${fixture} must remain explicit and only forward to Sage crates' fixture features.`,
    );
  }
  return errors;
}
