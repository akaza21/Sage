import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import {
  FROZEN_WORKBENCH_TRANSPORT_FIELDS,
  workbenchTransportParityErrors,
} from "./workbench-transport-parity.mjs";

const root = process.cwd();

test("frozen workbench transport types have an explicit field freeze", () => {
  assert.deepEqual(FROZEN_WORKBENCH_TRANSPORT_FIELDS.ResourceRef, [
    "relativePath",
    "rootId",
  ]);
  assert.deepEqual(FROZEN_WORKBENCH_TRANSPORT_FIELDS.ProjectTransitionResult, [
    "committed",
    "recoveryError",
    "snapshot",
    "transitionEpoch",
  ]);
  assert.ok(
    FROZEN_WORKBENCH_TRANSPORT_FIELDS.BootstrapSnapshot.includes(
      "transitionEpoch",
    ),
  );
  assert.ok(
    FROZEN_WORKBENCH_TRANSPORT_FIELDS.HarnessDefinition.includes("bindingId"),
  );
  assert.deepEqual(FROZEN_WORKBENCH_TRANSPORT_FIELDS.HarnessDefinition, [
    "accountModelDiscovery",
    "bindingId",
    "boundVersion",
    "characterisation",
    "description",
    "id",
    "installState",
    "name",
    "signIn",
    "source",
    "transport",
    "unavailableReason",
    "version",
    "versionChecking",
  ]);
  assert.ok(
    FROZEN_WORKBENCH_TRANSPORT_FIELDS.ModelOption.includes("isDefault"),
  );
});

test("Rust and TypeScript workbench contracts match the frozen transport fields", async () => {
  const errors = await workbenchTransportParityErrors(root);
  assert.deepEqual(errors, []);
});

test("workbench contracts split into owned modules with compatibility re-exports", async () => {
  const required = [
    "src/domain/resource.ts",
    "src/domain/project.ts",
    "src/domain/editor.ts",
    "src/domain/repository.ts",
    "src/domain/chat.ts",
    "src/domain/harness.ts",
    "src-tauri/src/domain/resource.rs",
    "src-tauri/src/domain/project.rs",
    "src-tauri/src/domain/editor.rs",
    "src-tauri/src/domain/repository.rs",
    "src-tauri/src/domain/chat.rs",
    "src-tauri/src/domain/harness.rs",
  ];
  for (const relative of required) {
    const source = await readFile(path.join(root, relative), "utf8");
    assert.match(source, /\S/u, relative);
  }
  const barrel = await readFile(
    path.join(root, "src/domain/workbench.ts"),
    "utf8",
  );
  for (const moduleName of [
    "resource",
    "project",
    "editor",
    "repository",
    "chat",
    "harness",
  ]) {
    assert.match(
      barrel,
      new RegExp(`export \\* from "\\./${moduleName}"`),
      `workbench.ts re-exports ${moduleName}`,
    );
  }
});
