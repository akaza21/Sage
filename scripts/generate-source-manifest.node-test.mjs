import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import {
  buildSourceManifest,
  validateEvidenceBinding,
} from "./generate-source-manifest.mjs";

function fixture() {
  const root = mkdtempSync(path.join(tmpdir(), "sage-manifest-"));
  mkdirSync(path.join(root, "src", "nested"), { recursive: true });
  writeFileSync(path.join(root, "package.json"), "{}\n");
  writeFileSync(path.join(root, "src", "z.ts"), "z\n");
  writeFileSync(path.join(root, "src", "nested", "a.ts"), "a\n");
  return root;
}

const policy = {
  files: ["package.json"],
  directories: ["src"],
  excludedSegments: ["target"],
};

test("source manifest is deterministic, normalized, ordered, and self-excluding", () => {
  const root = fixture();
  try {
    const first = buildSourceManifest(root, policy);
    writeFileSync(path.join(root, "source-manifest.json"), "ignored\n");
    const second = buildSourceManifest(root, policy);
    assert.equal(first.aggregateSha256, second.aggregateSha256);
    assert.deepEqual(
      first.files.map((file) => file.path),
      ["package.json", "src/nested/a.ts", "src/z.ts"],
    );
    assert.ok(first.files.every((file) => !file.path.includes("\\")));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("source manifest aggregate changes with a build input", () => {
  const root = fixture();
  try {
    const first = buildSourceManifest(root, policy);
    writeFileSync(path.join(root, "src", "z.ts"), "changed\n");
    const second = buildSourceManifest(root, policy);
    assert.notEqual(first.aggregateSha256, second.aggregateSha256);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("evidence binding rejects stale artifact and candidate labels", () => {
  const candidate = { aggregateSha256: "candidate" };
  const evidence = {
    artifacts: { final: { executableSha256: "final" } },
    candidate: {
      aggregateSha256: "old-candidate",
      executableSha256: "old-executable",
      campaigns: [
        {
          name: "interaction",
          aggregateSha256: "old-candidate",
          executableSha256: "old-executable",
        },
      ],
    },
  };
  assert.deepEqual(validateEvidenceBinding(candidate, evidence), [
    "evidence candidate aggregate does not match the source manifest",
    "candidate executable does not match the final artifact",
    "interaction uses a stale executable",
    "interaction uses a stale source candidate",
  ]);
});
