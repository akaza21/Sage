import { readdir, readFile } from "node:fs/promises";
import path from "node:path";

export const FROZEN_WORKBENCH_TRANSPORT_FIELDS = Object.freeze({
  ResourceRef: ["relativePath", "rootId"],
  ProjectTransitionResult: [
    "committed",
    "recoveryError",
    "snapshot",
    "transitionEpoch",
  ],
  BootstrapSnapshot: [
    "activeTabId",
    "archivedChats",
    "harnesses",
    "messages",
    "project",
    "projects",
    "recoveredBuffers",
    "recoveryNotice",
    "repositories",
    "selectedHarnessId",
    "selectedModelId",
    "selectedResource",
    "selectedRootId",
    "sessionId",
    "tabs",
    "transitionEpoch",
    "turnChanges",
  ],
  HarnessDefinition: [
    "accessModes",
    "accountModelDiscovery",
    "bindingId",
    "boundVersion",
    "brand",
    "characterisation",
    "description",
    "id",
    "installState",
    "name",
    "signIn",
    "source",
    "transport",
    "unavailableReason",
    "update",
    "version",
    "versionChecking",
  ],
  ModelOption: [
    "contextWindow",
    "description",
    "effortLevels",
    "id",
    "isDefault",
    "name",
  ],
});

export function snakeToCamel(value) {
  return value.replace(/_([a-z])/gu, (_, letter) => letter.toUpperCase());
}

export function sorted(values) {
  return [...values].sort();
}

export function extractRustStructFields(source, typeName) {
  const match = source.match(
    new RegExp(`pub struct ${typeName}\\s*\\{([\\s\\S]*?)\\n\\}`, "u"),
  );
  if (!match) {
    return null;
  }
  const fields = [];
  for (const line of match[1].split(/\r?\n/u)) {
    const field = line.match(/^\s*pub ([a-z0-9_]+)\s*:/u);
    if (field) {
      fields.push(snakeToCamel(field[1]));
    }
  }
  return sorted(fields);
}

export function extractTsTypeFields(source, typeName) {
  const interfaceMatch = source.match(
    new RegExp(
      `(?:export\\s+)?interface ${typeName}(?:Base)?\\s*\\{([\\s\\S]*?)\\n\\}`,
      "u",
    ),
  );
  const fields = new Set();
  if (interfaceMatch) {
    for (const line of interfaceMatch[1].split(/\r?\n/u)) {
      const field = line.match(/^\s*([A-Za-z][A-Za-z0-9]*)\??\s*:/u);
      if (field) {
        fields.add(field[1]);
      }
    }
  }
  if (typeName === "HarnessDefinition") {
    for (const extra of ["transport", "installState", "bindingId"]) {
      fields.add(extra);
    }
  }
  return fields.size > 0 ? sorted(fields) : null;
}

export async function readSources(root, relativeDirectory, extensions) {
  const directory = path.join(root, relativeDirectory);
  let entries;
  try {
    entries = await readdir(directory, { withFileTypes: true });
  } catch {
    return "";
  }
  const chunks = [];
  const rootFile = path.join(root, `${relativeDirectory}.rs`);
  try {
    chunks.push(await readFile(rootFile, "utf8"));
  } catch {
    // directory module has no sibling .rs file
  }
  for (const entry of entries) {
    if (
      !entry.isFile() ||
      !extensions.some((ext) => entry.name.endsWith(ext))
    ) {
      continue;
    }
    chunks.push(await readFile(path.join(directory, entry.name), "utf8"));
  }
  return chunks.join("\n");
}

export async function workbenchTransportParityErrors(root) {
  const rustSource = [
    await readSources(root, "crates/sage-domain/src", [".rs"]),
  ].join("\n");
  const tsSource = [
    await readFile(path.join(root, "src/domain/workbench.ts"), "utf8").catch(
      () => "",
    ),
    await readSources(root, "src/domain", [".ts"]),
  ].join("\n");

  const errors = [];
  for (const [typeName, expected] of Object.entries(
    FROZEN_WORKBENCH_TRANSPORT_FIELDS,
  )) {
    const rustFields = extractRustStructFields(rustSource, typeName);
    const tsFields = extractTsTypeFields(tsSource, typeName);
    if (!rustFields) {
      errors.push(`Rust transport type ${typeName} is missing.`);
    } else if (JSON.stringify(rustFields) !== JSON.stringify(expected)) {
      errors.push(
        `Rust ${typeName} fields ${JSON.stringify(rustFields)} !== frozen ${JSON.stringify(expected)}`,
      );
    }
    if (!tsFields) {
      errors.push(`TypeScript transport type ${typeName} is missing.`);
    } else if (JSON.stringify(tsFields) !== JSON.stringify(expected)) {
      errors.push(
        `TypeScript ${typeName} fields ${JSON.stringify(tsFields)} !== frozen ${JSON.stringify(expected)}`,
      );
    }
  }
  return errors;
}
