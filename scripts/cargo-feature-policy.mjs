// The app crate's features: nothing on by default, an exact custom-protocol
// feature for release builds, and an acp-fixture feature that may only turn
// on the `fixture` feature of Sage's own crates, never an external one.
function featureSection(source) {
  return source.match(/(?:^|\n)\[features\]\r?\n([\s\S]*?)(?=\r?\n\[|$)/)?.[1];
}

export function cargoFeaturePolicyErrors(source) {
  const features = featureSection(source);
  if (!features) return ["Cargo.toml must contain a [features] section."];
  const errors = [];
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
  const forward = '"sage-[a-z0-9-]+/fixture"';
  if (
    !new RegExp(
      `^acp-fixture\\s*=\\s*\\[\\s*(?:${forward}(?:\\s*,\\s*${forward})*\\s*,?\\s*)?\\]\\s*$`,
      "m",
    ).test(features)
  ) {
    errors.push(
      "Cargo fixture feature acp-fixture must remain explicit and only forward to Sage crates' fixture features.",
    );
  }
  return errors;
}
