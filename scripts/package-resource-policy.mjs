import { createHash } from "node:crypto";
import { lstat, readFile } from "node:fs/promises";
import path from "node:path";

// Production packages ship no resources or external binaries, with one
// curated exception: Pi loads extensions only from a file, so its approval
// bridge must ship beside Sage. The script holds no filesystem authority and
// runs Pi's own bash only after Sage records the user's approval; Rust
// re-checks these bytes before handing the path to Pi.
// Changing the script, or adding any other resource, must update this pin.
export const curatedPackageResources = Object.freeze([
  Object.freeze({
    path: "src/acp/bindings/pi/bridge/extension.mjs",
    bytes: 3324,
    sha256: "4329e9c722aff72df46d362340469261131849a457a925d937f26d6f7b52009d",
  }),
]);

export async function packageResourcePolicyErrors({
  tauriRoot,
  bundle,
  windowsBundle,
  curated = curatedPackageResources,
}) {
  const errors = [];
  const externalBinaries = [
    ...(bundle?.externalBin ?? []),
    ...(windowsBundle?.externalBin ?? []),
  ];
  if (externalBinaries.length !== 0) {
    errors.push("Production packages must not declare external binaries.");
  }
  if (windowsBundle?.resources !== undefined) {
    errors.push("The Windows overlay must not replace package resources.");
  }
  const declared = bundle?.resources ?? [];
  if (
    !Array.isArray(declared) ||
    JSON.stringify(declared) !==
      JSON.stringify(curated.map((resource) => resource.path))
  ) {
    errors.push(
      "Production package resources must be exactly the curated Pi bridge.",
    );
    return errors;
  }
  for (const resource of curated) {
    const absolute = path.join(tauriRoot, resource.path);
    try {
      const metadata = await lstat(absolute);
      if (!metadata.isFile() || metadata.isSymbolicLink()) {
        errors.push(`Curated package resource is not a file: ${resource.path}`);
        continue;
      }
      const bytes = await readFile(absolute);
      const sha256 = createHash("sha256").update(bytes).digest("hex");
      if (bytes.length !== resource.bytes || sha256 !== resource.sha256) {
        errors.push(`Curated package resource drifted: ${resource.path}`);
      }
    } catch {
      errors.push(`Curated package resource is missing: ${resource.path}`);
    }
  }
  return errors;
}
