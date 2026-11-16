// Test helpers never reach a shipped or build-time binary (spec 4.4 rule 4 and
// 7.5). These rules read the resolved `cargo metadata`, so workspace
// inheritance, renames, dotted keys, string escapes and overrides are already
// applied.

const TEST_SUPPORT_CRATE = "sage-test-support";

// Manifests whose own `test-support` feature may pull in sage-test-support as
// an optional dependency. Each entry is an owner decision.
const OPTIONAL_TEST_SUPPORT_MANIFESTS = new Set(["crates/sage-git/Cargo.toml"]);

const forwardedTestSupport = /^[A-Za-z0-9_-]+\??\/test-support$/u;
const enablesTestSupport = /(?:^|\/)test-support$/u;

function slashed(file) {
  return file.replaceAll("\\", "/");
}

function dependencyKey(dependency) {
  return dependency.rename ?? dependency.name;
}

function namesDependency(value, key) {
  return (
    value === `dep:${key}` ||
    value.startsWith(`${key}/`) ||
    value.startsWith(`${key}?/`)
  );
}

export function testSupportGraphErrors(metadata) {
  const root = slashed(metadata.workspace_root);
  const members = new Set(metadata.workspace_members);
  const names = new Map(metadata.packages.map((pkg) => [pkg.id, pkg.name]));
  const localOutsiders = metadata.packages
    .filter((pkg) => pkg.source === null && !members.has(pkg.id))
    .map(
      (pkg) => `${pkg.name} is a local crate outside the workspace members.`,
    );
  const edgesIntoMembers = metadata.resolve.nodes
    .filter((node) => !members.has(node.id))
    .flatMap((node) =>
      node.deps
        .filter((dep) => members.has(dep.pkg))
        .map(
          (dep) =>
            `${names.get(node.id)} is outside the workspace members and may not depend on ${names.get(dep.pkg)}.`,
        ),
    );
  const memberErrors = metadata.packages
    .filter((pkg) => members.has(pkg.id))
    .flatMap((pkg) => {
      const manifest = slashed(pkg.manifest_path);
      const relative = manifest.startsWith(`${root}/`)
        ? manifest.slice(root.length + 1)
        : manifest;
      return packageErrors(pkg, OPTIONAL_TEST_SUPPORT_MANIFESTS.has(relative));
    });
  return [...localOutsiders, ...edgesIntoMembers, ...memberErrors];
}

function packageErrors(pkg, mayTakeTestSupport) {
  const errors = [];
  const testSupportFeature = pkg.features["test-support"] ?? [];
  const helperKeys = pkg.dependencies
    .filter((dependency) => dependency.name === TEST_SUPPORT_CRATE)
    .map(dependencyKey);
  for (const dependency of pkg.dependencies) {
    if (dependency.kind === "dev") continue;
    const key = dependencyKey(dependency);
    if (dependency.features.includes("test-support")) {
      errors.push(
        `${pkg.name} may enable ${key}'s test-support feature only from [dev-dependencies].`,
      );
    }
    const featureGated =
      mayTakeTestSupport &&
      dependency.kind === null &&
      dependency.target === null &&
      dependency.optional &&
      testSupportFeature.includes(`dep:${key}`);
    if (dependency.name === TEST_SUPPORT_CRATE && !featureGated) {
      errors.push(
        `${pkg.name} may take ${TEST_SUPPORT_CRATE} only as a dev-dependency.`,
      );
    }
  }
  for (const [feature, values] of Object.entries(pkg.features)) {
    for (const value of values) {
      const allowed =
        feature === "test-support"
          ? forwardedTestSupport.test(value) ||
            (mayTakeTestSupport &&
              helperKeys.some((key) => value === `dep:${key}`))
          : !enablesTestSupport.test(value) &&
            !helperKeys.some((key) => namesDependency(value, key));
      if (!allowed) {
        errors.push(
          `${pkg.name}'s ${feature} feature may not enable ${value}.`,
        );
      }
    }
  }
  return errors;
}
