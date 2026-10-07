import { readFile } from "node:fs/promises";
import path from "node:path";

import {
  extractRustStructFields,
  extractTsTypeFields,
  readSources,
} from "./workbench-transport-parity.mjs";

export const FROZEN_ACP_TRANSPORT_FIELDS = Object.freeze({
  AcpCapabilitySet: [
    "activeTurnSteer",
    "cancel",
    "childConversationProjection",
    "childTranscriptRead",
    "closeSession",
    "collaborationEventProjection",
    "configCount",
    "loadSession",
    "modelCount",
    "multiRootExtensionSchemaId",
    "nativeSessionFork",
    "newSession",
    "prompt",
    "protocolVersion",
    "resumeSession",
    "schemaVersion",
  ],
  AcpItemIdentity: [
    "activityId",
    "chatId",
    "createdAt",
    "id",
    "lifecycle",
    "ordinal",
    "origin",
    "projectId",
    "revision",
    "runtimeEpoch",
    "sessionId",
    "terminalAt",
    "turnId",
    "updatedAt",
  ],
  BetaEvidenceResult: [
    "candidate",
    "contractSha256",
    "evidence",
    "schemaVersion",
    "verdict",
    "workItem",
  ],
});

const RUST_TYPE_ALIASES = Object.freeze({
  AcpCapabilitySet: "CapabilitySnapshotV1",
  AcpItemIdentity: "ItemProjection",
  BetaEvidenceResult: "BetaEvidenceResult",
});

const RUST_IGNORED_FIELDS = Object.freeze({
  ItemProjection: ["content", "toolState"],
});

export async function acpTransportParityErrors(root) {
  const rustSource = [
    await readFile(
      path.join(root, "src-tauri/src/acp/intent.rs"),
      "utf8",
    ).catch(() => ""),
    await readSources(root, "src-tauri/src/acp/intent", [".rs"]),
    await readFile(
      path.join(root, "src-tauri/src/acp/projection.rs"),
      "utf8",
    ).catch(() => ""),
    await readFile(path.join(root, "src-tauri/src/domain.rs"), "utf8").catch(
      () => "",
    ),
    await readSources(root, "src-tauri/src/domain", [".rs"]),
  ].join("\n");
  const tsSource = [
    await readFile(path.join(root, "src/domain/acp.ts"), "utf8").catch(
      () => "",
    ),
    await readSources(root, "src/domain", [".ts"]),
  ].join("\n");

  const errors = [];
  for (const [typeName, expected] of Object.entries(
    FROZEN_ACP_TRANSPORT_FIELDS,
  )) {
    const rustName = RUST_TYPE_ALIASES[typeName] ?? typeName;
    const ignored = new Set(RUST_IGNORED_FIELDS[rustName] ?? []);
    const rustFields = extractRustStructFields(rustSource, rustName)?.filter(
      (field) => !ignored.has(field),
    );
    const tsFields = extractTsTypeFields(tsSource, typeName);
    if (!rustFields) {
      errors.push(`Rust transport type ${rustName} is missing.`);
    } else if (JSON.stringify(rustFields) !== JSON.stringify(expected)) {
      errors.push(
        `Rust ${rustName} fields ${JSON.stringify(rustFields)} !== frozen ${JSON.stringify(expected)}`,
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
