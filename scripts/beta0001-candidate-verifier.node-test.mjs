import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  link,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  symlink,
  unlink,
  utimes,
  writeFile,
} from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  betaCandidateRelativePathComponents,
  readBoundedPlainEvidenceFile,
} from "./beta0001-candidate-bounds.mjs";
import { validateBetaCandidateEvidence } from "./beta0001-candidate-verifier.mjs";

const repositoryRoot = process.cwd();
const thresholdPath = path.join(
  repositoryRoot,
  "docs/work-items/BETA-0001-primary-beta-path/contracts/beta-acceptance-v1.json",
);
const sourceCommit = "a".repeat(40);
const ids = [
  "windows.source-package",
  "windows.installed-lifecycle",
  "projects.workbench-multiroot",
  "resources.explorer",
  "editor.file-diff-recovery-find",
  "search.workspace",
  "git.basic",
  "chat.lifecycle-handoff",
  "acp.durable-edit-summary",
  "codex.account-models",
  "codex.multichat-turn-restart-cancel",
  "codex.process-convergence",
  "renderer.a11y-reload-focus",
  "security.authority-secrets",
  "recovery.crash-cleanup",
  "performance.resource-storage",
  "macos.shared-contract-ci",
  "review.standards",
  "review.spec",
  "test.independent",
];

async function thresholdAuthority() {
  const bytes = await readFile(thresholdPath);
  return {
    value: JSON.parse(bytes),
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
}

function candidate(commit = sourceCommit) {
  return { platform: "windows", architecture: "x86_64", sourceCommit: commit };
}

function metricDefinitions(threshold) {
  const journey = threshold.codexJourney;
  const budgets = threshold.codexBudgets;
  const minimum = (name, ceiling) => ({
    name,
    value: ceiling,
    within: (value) => value >= ceiling,
  });
  const maximum = (name, ceiling) => ({
    name,
    value: ceiling,
    within: (value) => value <= ceiling,
  });
  return new Map([
    [
      "codex.account-models",
      [minimum("observedModels", journey.minimumObservedModels)],
    ],
    [
      "codex.multichat-turn-restart-cancel",
      [
        minimum("distinctChats", journey.minimumDistinctChats),
        minimum(
          "sequentialTurnsInOneSession",
          journey.minimumSequentialTurnsInOneSession,
        ),
        minimum("successfulTurns", journey.minimumSuccessfulTurns),
        minimum(
          "restartResumeThenTurnCycles",
          journey.minimumRestartResumeThenTurnCycles,
        ),
        minimum("cancelThenReuseCycles", journey.minimumCancelThenReuseCycles),
        minimum("cleanCycles", journey.minimumCleanCycles),
        minimum("failureCycles", journey.minimumFailureCycles),
      ],
    ],
    [
      "codex.process-convergence",
      [
        maximum(
          "orphanProcessesAfterClose",
          journey.maximumOrphanProcessesAfterClose,
        ),
      ],
    ],
    [
      "security.authority-secrets",
      [
        maximum(
          "secretExposureObservations",
          journey.maximumSecretExposureObservations,
        ),
        maximum(
          "unauthorizedMutationObservations",
          journey.maximumUnauthorizedMutationObservations,
        ),
        maximum(
          "unexpectedListenerCount",
          budgets.unexpectedListenerCountMaximum,
        ),
        maximum(
          "unexpectedEnvironmentEntryCount",
          budgets.unexpectedEnvironmentEntryCountMaximum,
        ),
      ],
    ],
    [
      "performance.resource-storage",
      Object.entries(budgets)
        .map(([key, value]) =>
          key === "unexpectedListenerCountMaximum" ||
          key === "unexpectedEnvironmentEntryCountMaximum"
            ? null
            : key === "minimumLatencySamples"
              ? minimum("latencySamples", value)
              : maximum(key.replace(/Maximum$/u, ""), value),
        )
        .filter(Boolean),
    ],
  ]);
}

function metricsFor(id, definitions, mutate) {
  return Object.fromEntries(
    (definitions.get(id) ?? []).map((definition) => [
      definition.name,
      mutate?.id === id && mutate.name === definition.name
        ? mutate.value
        : definition.value,
    ]),
  );
}

async function fixture({
  statuses = {},
  omit = [],
  aggregate = {},
  document = {},
  mutate,
} = {}) {
  const root = await mkdtemp(path.join(os.tmpdir(), "beta0001-candidate-"));
  const { value: threshold, sha256 } = await thresholdAuthority();
  const definitions = metricDefinitions(threshold);
  const descriptors = ids.map((id) => ({
    id,
    status: statuses[id] ?? (omit.includes(id) ? "missing" : "passed"),
  }));
  for (const descriptor of descriptors) {
    if (omit.includes(descriptor.id)) continue;
    const documentValue = {
      schemaVersion: 1,
      evidenceId: descriptor.id,
      contractSha256: sha256,
      candidate: candidate(),
      status: statuses[descriptor.id] ?? "passed",
      metrics: metricsFor(descriptor.id, definitions, mutate),
      ...(document[descriptor.id] ?? {}),
    };
    await writeFile(
      path.join(root, `${descriptor.id}.json`),
      JSON.stringify(documentValue),
    );
  }
  const result = {
    schemaVersion: 1,
    workItem: "BETA-0001",
    contractSha256: sha256,
    candidate: candidate(),
    evidence: descriptors,
    verdict: "passed",
    ...aggregate,
  };
  return {
    root,
    threshold,
    sha256,
    definitions,
    result,
    bytes: Buffer.from(JSON.stringify(result)),
  };
}

async function close(fixtureValue) {
  await rm(fixtureValue.root, { recursive: true, force: true });
}
async function validate(value) {
  return validateBetaCandidateEvidence({
    resultBytes: value.bytes,
    evidenceRoot: value.root,
    expectedSourceCommit: sourceCommit,
  });
}
async function rejectsCode(action, code) {
  await assert.rejects(
    Promise.resolve().then(action),
    (error) => error?.code === code,
  );
}
async function padEvidenceAggregate(value, total) {
  let remaining = total;
  const files = [];
  for (const id of ids) {
    const file = path.join(value.root, `${id}.json`);
    const bytes = await readFile(file);
    files.push({ file, bytes });
    remaining -= bytes.length;
  }
  for (const entry of files) {
    const padding = Math.min(remaining, 1_048_576 - entry.bytes.length);
    if (padding > 0)
      await writeFile(
        entry.file,
        Buffer.concat([entry.bytes, Buffer.alloc(padding, 0x20)]),
      );
    remaining -= padding;
  }
  assert.equal(remaining, 0, "fixture can reach requested aggregate size");
}

test("complete exact candidate evidence derives a passing verdict", async () => {
  const value = await fixture();
  try {
    const result = await validate(value);
    assert.equal(result.verdict, "passed");
    assert.equal(result.contractSha256, value.sha256);
  } finally {
    await close(value);
  }
});

test("isolated journey failure derives failed, not malformed", async () => {
  const value = await fixture({
    statuses: { "git.basic": "failed" },
    aggregate: { verdict: "failed" },
  });
  try {
    assert.equal((await validate(value)).verdict, "failed");
  } finally {
    await close(value);
  }
});

test("isolated maximum overflow derives failed, not malformed", async () => {
  const base = await fixture();
  const target = base.definitions.get("codex.process-convergence")[0];
  await close(base);
  const value = await fixture({
    mutate: {
      id: "codex.process-convergence",
      name: target.name,
      value: target.value + 1,
    },
    aggregate: { verdict: "failed" },
  });
  try {
    assert.equal((await validate(value)).verdict, "failed");
  } finally {
    await close(value);
  }
});

test("isolated minimum underflow derives failed, not malformed", async () => {
  const base = await fixture();
  const target = base.definitions.get("codex.account-models")[0];
  await close(base);
  const value = await fixture({
    mutate: {
      id: "codex.account-models",
      name: target.name,
      value: target.value - 1,
    },
    aggregate: { verdict: "failed" },
  });
  try {
    assert.equal((await validate(value)).verdict, "failed");
  } finally {
    await close(value);
  }
});

test("missing required evidence is incomplete", async () => {
  const value = await fixture({
    omit: [ids[0]],
    aggregate: { verdict: "incomplete" },
  });
  try {
    assert.equal((await validate(value)).verdict, "incomplete");
  } finally {
    await close(value);
  }
});

test("absent file with passed descriptor is status-drift", async () => {
  const value = await fixture({
    omit: [ids[0]],
    statuses: { [ids[0]]: "passed" },
  });
  try {
    await rejectsCode(() => validate(value), "status-drift");
  } finally {
    await close(value);
  }
});

test("present file with missing descriptor is status-drift", async () => {
  const value = await fixture({ statuses: { [ids[0]]: "missing" } });
  try {
    await rejectsCode(() => validate(value), "status-drift");
  } finally {
    await close(value);
  }
});

test("duplicate unknown reordered and wrong-count evidence are rejected", async () => {
  for (const [change, code] of [
    [
      (result) => {
        result.evidence[1] = { ...result.evidence[0] };
      },
      "duplicate-evidence",
    ],
    [
      (result) => {
        result.evidence[0] = { ...result.evidence[0], id: "not.required" };
      },
      "unknown-evidence",
    ],
    [
      (result) => {
        [result.evidence[0], result.evidence[1]] = [
          result.evidence[1],
          result.evidence[0],
        ];
      },
      "evidence-order",
    ],
    [
      (result) => {
        result.evidence.pop();
      },
      "evidence-count",
    ],
  ]) {
    const value = await fixture();
    change(value.result);
    value.bytes = Buffer.from(JSON.stringify(value.result));
    try {
      await rejectsCode(() => validate(value), code);
    } finally {
      await close(value);
    }
  }
});

test("expected commit mismatch is candidate-stale and document mismatch is candidate-drift", async () => {
  const stale = await fixture();
  stale.result.candidate = candidate("b".repeat(40));
  stale.bytes = Buffer.from(JSON.stringify(stale.result));
  try {
    await rejectsCode(() => validate(stale), "candidate-stale");
  } finally {
    await close(stale);
  }
  const drift = await fixture({
    document: { [ids[0]]: { candidate: candidate("b".repeat(40)) } },
  });
  try {
    await rejectsCode(() => validate(drift), "candidate-drift");
  } finally {
    await close(drift);
  }
});

test("contract-hash drift is rejected", async () => {
  const value = await fixture();
  value.result.contractSha256 = "0".repeat(64);
  value.bytes = Buffer.from(JSON.stringify(value.result));
  try {
    await rejectsCode(() => validate(value), "contract-drift");
  } finally {
    await close(value);
  }
});

test("submitted verdict drift is rejected", async () => {
  const value = await fixture({ aggregate: { verdict: "failed" } });
  try {
    await rejectsCode(() => validate(value), "status-drift");
  } finally {
    await close(value);
  }
});

test("extra or missing schema keys are schema-invalid", async () => {
  const extra = await fixture({ aggregate: { extra: true } });
  try {
    await rejectsCode(() => validate(extra), "schema-invalid");
  } finally {
    await close(extra);
  }
  const missing = await fixture();
  delete missing.result.workItem;
  missing.bytes = Buffer.from(JSON.stringify(missing.result));
  try {
    await rejectsCode(() => validate(missing), "schema-invalid");
  } finally {
    await close(missing);
  }
});

test("nonfinite negative and unsafe-integer metrics are metric-invalid", async () => {
  for (const mutate of [
    { id: "codex.account-models", name: "observedModels", value: -1 },
    {
      id: "codex.account-models",
      name: "observedModels",
      value: 9_007_199_254_740_992,
    },
  ]) {
    const value = await fixture({ mutate });
    try {
      await rejectsCode(() => validate(value), "metric-invalid");
    } finally {
      await close(value);
    }
  }
  const nonfinite = await fixture();
  const evidencePath = path.join(nonfinite.root, "codex.account-models.json");
  await writeFile(
    evidencePath,
    (await readFile(evidencePath, "utf8")).replace(
      '"observedModels":1',
      '"observedModels":1e400',
    ),
  );
  try {
    await rejectsCode(() => validate(nonfinite), "metric-invalid");
  } finally {
    await close(nonfinite);
  }
});

test("unsafe linked junction hardlink escaped and Windows-alias paths never enter the candidate", async () => {
  const value = await fixture();
  const linkedRoot = `${value.root}-link`;
  await symlink(value.root, linkedRoot, "junction");
  try {
    await rejectsCode(
      () =>
        validateBetaCandidateEvidence({
          resultBytes: value.bytes,
          evidenceRoot: linkedRoot,
          expectedSourceCommit: sourceCommit,
        }),
      "evidence-link",
    );
  } finally {
    await rm(linkedRoot, { recursive: true, force: true });
    await close(value);
  }
  const linked = await fixture();
  await unlink(path.join(linked.root, `${ids[0]}.json`));
  await symlink(
    path.join(linked.root, `${ids[1]}.json`),
    path.join(linked.root, `${ids[0]}.json`),
  );
  try {
    await rejectsCode(() => validate(linked), "evidence-link");
  } finally {
    await close(linked);
  }
  const hardlinked = await fixture();
  await link(
    path.join(hardlinked.root, `${ids[1]}.json`),
    path.join(hardlinked.root, "extra.json"),
  );
  try {
    await rejectsCode(() => validate(hardlinked), "evidence-link");
  } finally {
    await close(hardlinked);
  }
  for (const unsafe of ["a:b", "CON", "COM¹", "LPT²", "name.", "name "])
    await rejectsCode(
      () => betaCandidateRelativePathComponents(unsafe, { maximumBytes: 512 }),
      "evidence-path-invalid",
    );
  const paths = await fixture();
  try {
    await rejectsCode(
      () =>
        readBoundedPlainEvidenceFile({
          root: paths.root,
          id: "git.basic/ignored",
          maximumBytes: 1,
        }),
      "evidence-path-invalid",
    );
  } finally {
    await close(paths);
  }
});

test("ordinary evidence directory with structure link count reads successfully", async () => {
  const root = await mkdtemp(
    path.join(os.tmpdir(), "beta0001-candidate-ordinary-dir-"),
  );
  const id = "git.basic";
  await writeFile(path.join(root, `${id}.json`), "{}");
  // An ordinary directory's link count reflects filesystem structure, not a
  // regular-file hardlink: entering a subdirectory must not reject the root.
  await mkdir(path.join(root, "structure"));
  try {
    const bytes = await readBoundedPlainEvidenceFile({
      root,
      id,
      maximumBytes: 2,
    });
    assert.equal(bytes.toString("utf8"), "{}");
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("identity replacement during read is evidence-identity-changed", async () => {
  const root = await mkdtemp(
    path.join(os.tmpdir(), "beta0001-candidate-read-"),
  );
  const id = "git.basic";
  const file = path.join(root, `${id}.json`);
  await writeFile(file, "{}");
  try {
    await rejectsCode(
      () =>
        readBoundedPlainEvidenceFile({
          root,
          id,
          maximumBytes: 2,
          onAfterOpen: async () => {
            await unlink(file);
            await writeFile(file, "{}");
          },
        }),
      "evidence-identity-changed",
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("evidence root metadata change during read is rejected", async () => {
  const root = await mkdtemp(
    path.join(os.tmpdir(), "beta0001-candidate-root-read-"),
  );
  const id = "git.basic";
  await writeFile(path.join(root, `${id}.json`), "{}");
  try {
    await rejectsCode(
      () =>
        readBoundedPlainEvidenceFile({
          root,
          id,
          maximumBytes: 2,
          onAfterOpen: async () => {
            await utimes(root, new Date(0), new Date(0));
          },
        }),
      "evidence-identity-changed",
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("file disappearance after first snapshot is evidence-identity-changed, not incomplete", async () => {
  const root = await mkdtemp(
    path.join(os.tmpdir(), "beta0001-candidate-disappear-"),
  );
  const id = "git.basic";
  const file = path.join(root, `${id}.json`);
  await writeFile(file, "{}");
  try {
    await rejectsCode(
      () =>
        readBoundedPlainEvidenceFile({
          root,
          id,
          maximumBytes: 2,
          onAfterOpen: () => unlink(file),
        }),
      "evidence-identity-changed",
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("result evidence and aggregate-byte ceilings accept exact and reject plus one", async () => {
  const exact = await fixture();
  exact.bytes = Buffer.concat([
    exact.bytes,
    Buffer.alloc(262_144 - exact.bytes.length, 0x20),
  ]);
  try {
    assert.equal((await validate(exact)).verdict, "passed");
  } finally {
    await close(exact);
  }
  const plus = await fixture();
  plus.bytes = Buffer.concat([
    plus.bytes,
    Buffer.alloc(262_145 - plus.bytes.length, 0x20),
  ]);
  try {
    await rejectsCode(() => validate(plus), "result-oversize");
  } finally {
    await close(plus);
  }
  const evidence = await fixture();
  const evidenceFile = path.join(evidence.root, `${ids[0]}.json`);
  const evidenceBytes = await readFile(evidenceFile);
  await writeFile(
    evidenceFile,
    Buffer.concat([
      evidenceBytes,
      Buffer.alloc(1_048_576 - evidenceBytes.length, 0x20),
    ]),
  );
  try {
    assert.equal((await validate(evidence)).verdict, "passed");
  } finally {
    await close(evidence);
  }
  const evidencePlus = await fixture();
  const plusFile = path.join(evidencePlus.root, `${ids[0]}.json`);
  const plusBytes = await readFile(plusFile);
  await writeFile(
    plusFile,
    Buffer.concat([
      plusBytes,
      Buffer.alloc(1_048_577 - plusBytes.length, 0x20),
    ]),
  );
  try {
    await rejectsCode(() => validate(evidencePlus), "evidence-oversize");
  } finally {
    await close(evidencePlus);
  }
  const aggregate = await fixture();
  await padEvidenceAggregate(aggregate, 4_194_304);
  try {
    assert.equal((await validate(aggregate)).verdict, "passed");
  } finally {
    await close(aggregate);
  }
  const aggregatePlus = await fixture();
  await padEvidenceAggregate(aggregatePlus, 4_194_305);
  try {
    await rejectsCode(
      () => validate(aggregatePlus),
      "evidence-aggregate-oversize",
    );
  } finally {
    await close(aggregatePlus);
  }
});

test("JSON depth node and string ceilings accept exact and reject plus one", async () => {
  const { inspectBetaCandidateJson } =
    await import("./beta0001-candidate-bounds.mjs");
  const limits = {
    jsonDepthMaximum: 32,
    jsonNodesMaximum: 16384,
    stringBytesMaximum: 4096,
  };
  let depthExact = 0;
  for (let index = 0; index < 31; index += 1) depthExact = [depthExact];
  inspectBetaCandidateJson(depthExact, limits);
  await rejectsCode(
    () => inspectBetaCandidateJson([depthExact], limits),
    "json-depth",
  );
  inspectBetaCandidateJson("x".repeat(4096), limits);
  await rejectsCode(
    () => inspectBetaCandidateJson("x".repeat(4097), limits),
    "json-string",
  );
  inspectBetaCandidateJson(
    Array.from({ length: 16383 }, () => 0),
    limits,
  );
  await rejectsCode(
    () =>
      inspectBetaCandidateJson(
        Array.from({ length: 16384 }, () => 0),
        limits,
      ),
    "json-nodes",
  );
});

test("relative path helper accepts 512 bytes and rejects 513 plus aliases", async () => {
  assert.deepEqual(
    betaCandidateRelativePathComponents("a".repeat(512), { maximumBytes: 512 }),
    ["a".repeat(512)],
  );
  await rejectsCode(
    () =>
      betaCandidateRelativePathComponents("a".repeat(513), {
        maximumBytes: 512,
      }),
    "evidence-path-invalid",
  );
});
