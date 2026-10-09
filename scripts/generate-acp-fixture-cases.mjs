import { nativeCases } from "./acp-native-cases.mjs";
import { piIdleConfigJourney } from "./acp-native-pi-cases.mjs";
import { privacyFailuresForRecords } from "./check-harness-trace-privacy.mjs";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const workspace = join(dirname(fileURLToPath(import.meta.url)), "..");
const sourcePath = join(
  workspace,
  "docs",
  "work-items",
  "ADE-0006A-acp-session-foundation",
  "contracts",
  "fixture-vectors-v1.json",
);
const outputPath = join(
  workspace,
  "src-tauri",
  "src",
  "acp",
  "fixture",
  "generated_cases.rs",
);
const contractsPath = dirname(sourcePath);
const limits = JSON.parse(
  readFileSync(join(contractsPath, "limits-v1.json"), "utf8"),
);
const limitCoverage = JSON.parse(
  readFileSync(join(contractsPath, "limit-fixture-coverage-v1.json"), "utf8"),
);

const source = readFileSync(sourcePath);
const document = JSON.parse(source.toString("utf8"));
if (!Array.isArray(document.cases) || document.cases.length !== 179) {
  throw new Error("fixture contract must contain exactly 179 cases");
}

const execution = document.executionContract;
const childOperations = new Set(execution?.childOperations);
const hostOperations = new Set(execution?.hostOperations);
if (
  childOperations.size !== 22 ||
  hostOperations.size !== 64 ||
  [...childOperations].some((operation) => hostOperations.has(operation))
) {
  throw new Error(
    "fixture execution operation ownership must be closed and disjoint",
  );
}
const phaseIds = {
  windows: new Set(execution.phaseExceptions.windows),
  package: new Set(execution.phaseExceptions.package),
  deferredFoundation: new Set(execution.phaseExceptions.deferredFoundation),
};

const identifiers = new Set();
const casesById = new Map();
const ownershipCounts = { Child: 0, Host: 0 };
const phaseCounts = { Foundation: 0, Windows: 0, Package: 0, Deferred: 0 };
const rows = document.cases.map((fixtureCase) => {
  if (
    typeof fixtureCase.id !== "string" ||
    fixtureCase.id.length === 0 ||
    identifiers.has(fixtureCase.id) ||
    !Array.isArray(fixtureCase.steps) ||
    fixtureCase.steps.length === 0 ||
    fixtureCase.oracle === null ||
    Array.isArray(fixtureCase.oracle) ||
    typeof fixtureCase.oracle !== "object"
  ) {
    throw new Error(`invalid frozen fixture case ${String(fixtureCase.id)}`);
  }
  identifiers.add(fixtureCase.id);
  casesById.set(fixtureCase.id, fixtureCase);
  const operations = new Set(fixtureCase.steps.map((step) => step?.op));
  const hasChild = [...operations].some((operation) =>
    childOperations.has(operation),
  );
  const hasHost = [...operations].some((operation) =>
    hostOperations.has(operation),
  );
  const unknown = [...operations].filter(
    (operation) =>
      !childOperations.has(operation) && !hostOperations.has(operation),
  );
  if (unknown.length > 0 || hasChild === hasHost) {
    throw new Error(
      `fixture case ${fixtureCase.id} has unknown or mixed execution ownership`,
    );
  }
  const owner = hasChild ? "Child" : "Host";
  ownershipCounts[owner] += 1;
  const matchedPhases = Object.entries(phaseIds)
    .filter(([, ids]) => ids.has(fixtureCase.id))
    .map(([phase]) => phase);
  if (matchedPhases.length > 1) {
    throw new Error(
      `fixture case ${fixtureCase.id} belongs to multiple acceptance phases`,
    );
  }
  const phase =
    matchedPhases[0] === "windows"
      ? "Windows"
      : matchedPhases[0] === "package"
        ? "Package"
        : matchedPhases[0] === "deferredFoundation"
          ? "Deferred"
          : "Foundation";
  phaseCounts[phase] += 1;
  const steps = JSON.stringify(fixtureCase.steps);
  const oracle = JSON.stringify(fixtureCase.oracle);
  for (const value of [fixtureCase.id, steps, oracle]) {
    if (value.includes('"#')) {
      throw new Error(
        `fixture case ${fixtureCase.id} cannot use the Rust raw-string terminator`,
      );
    }
  }
  return `    FrozenCase { id: ${JSON.stringify(fixtureCase.id)}, steps_json: r#"${steps}"#, owner: FrozenOwner::${owner}, phase: FrozenPhase::${phase}, #[cfg(test)] oracle_json: r#"${oracle}"# },`;
});

const journeyCases = [
  {
    id: "codex-steer-lost-result",
    steps: [
      { op: "awaitClient", kind: "codexInitialize" },
      { op: "emitKnownFrame", shape: "codexInitializeResult0146" },
      { op: "awaitClient", kind: "codexStart" },
      { op: "emitKnownFrame", shape: "codexThreadResult" },
      { op: "awaitClient", kind: "codexGoalGetWithOptionalModelList" },
      { op: "emitKnownFrame", shape: "codexGoalMethodNotFound" },
      { op: "awaitClient", kind: "codexTurnStart" },
      { op: "emitKnownFrame", shape: "codexTurnResult" },
      { op: "emitKnownFrame", shape: "codexStarted" },
      { op: "awaitClient", kind: "codexSteer" },
      { op: "exit", code: 17 },
    ],
  },
];
journeyCases.push(...nativeCases());
journeyCases.push(piIdleConfigJourney());
if (privacyFailuresForRecords(journeyCases, false).length)
  throw new Error("native fixture privacy violation");
const journeyIds = new Set();
for (const fixtureCase of journeyCases) {
  if (
    typeof fixtureCase.id !== "string" ||
    fixtureCase.id.length === 0 ||
    identifiers.has(fixtureCase.id) ||
    journeyIds.has(fixtureCase.id) ||
    fixtureCase.steps.length > limits.fixture.stepsPerCase ||
    fixtureCase.steps.some((step) => !childOperations.has(step.op)) ||
    JSON.stringify(fixtureCase.steps).includes('"#')
  ) {
    throw new Error(`invalid live journey fixture ${fixtureCase.id}`);
  }
  journeyIds.add(fixtureCase.id);
}
const journeyRows = journeyCases.map(
  ({ id, steps }) =>
    `    JourneyCase { id: ${JSON.stringify(id)}, steps_json: r#"${JSON.stringify(steps)}"# },`,
);

function limitAt(path) {
  const value = path
    .split(".")
    .reduce((current, key) => current?.[key], limits);
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error(`unknown or nonnumeric frozen limit ${path}`);
  }
  return value;
}

function one(steps, operation) {
  if (steps.length !== 1 || steps[0]?.op !== operation) {
    throw new Error(`expected one ${operation} step`);
  }
  return steps[0];
}

function sum(steps, field) {
  return steps.reduce((total, step) => total + Number(step[field] ?? 0), 0);
}

const directMeasurements = {
  "wire.frameBytes": ["emitSizedFrame", "bytes"],
  "wire.jsonDepth": ["emitBoundedJson", "depth"],
  "wire.jsonNodes": ["emitBoundedJson", "count"],
  "wire.objectMembers": ["emitBoundedJson", "count"],
  "wire.arrayEntries": ["emitBoundedJson", "count"],
  "wire.decodedKeyBytes": ["emitBoundedJson", "bytes"],
  "wire.methodBytes": ["emitRpcMethod", "bytes"],
  "wire.numericTokenBytes": ["emitBoundedJson", "bytes"],
  "wire.externalIdBytes": ["emitExternalId", "bytes"],
  "wire.pageCursorBytes": ["readPage", "cursorBytes"],
  "wire.metadataEntries": ["emitMetadata", "entries"],
  "wire.absolutePathBytesNativeOnly": ["mapNativePath", "absolutePathBytes"],
  "wire.relativePathBytesNativeOnly": ["mapNativePath", "relativePathBytes"],
  "wire.promptBytes": ["sendPrompt", "bytes"],
  "wire.capabilityMetadataBytes": ["emitCapabilityToken", "totalTokenBytes"],
  "wire.modelsOrConfigEntries": ["emitModels", "entries"],
  "wire.normalizedItemBytes": ["normalizeItem", "bytes"],
  "wire.retainedToolInputBytes": ["normalizeTool", "inputBytes"],
  "wire.retainedToolOutputBytes": ["normalizeTool", "outputBytes"],
  "wire.permissionPayloadBytes": ["emitPermissionPayload", "bytes"],
  "wire.errorBytes": ["normalizeError", "bytes"],
  "wire.unsupportedSummaryBytes": ["normalizeUnsupported", "bytes"],
  "wire.itemsPerTurn": ["persistItems", "count"],
  "queues.driverDataMessages": ["fillDriverData", "messages"],
  "queues.outstandingRpcPerSession": ["reserveRpc", "perSession"],
  "queues.outstandingRpcGlobal": ["reserveRpc", "global"],
  "queues.serverRequestsPerSession": ["reserveServerRequests", "perSession"],
  "queues.serverRequestsGlobal": ["reserveServerRequests", "global"],
  "queues.runtimeTasksPerSession": ["reserveRuntimeTasks", "perSession"],
  "queues.runtimeTasksGlobal": ["reserveRuntimeTasks", "global"],
  "process.arguments": ["launchShape", "argumentCount"],
  "process.environmentEntries": ["launchShape", "environmentEntries"],
  "process.environmentKeyBytes": ["launchShape", "environmentKeyBytes"],
  "process.environmentValueBytes": ["launchShape", "environmentValueBytes"],
  "process.stderrLineBytes": ["emitStderrLine", "bytes"],
  "process.terminalTailBytes": ["retainTerminalTail", "bytes"],
  "nativeThreadProjection.activityGroupsPerTurn": [
    "persistActivities",
    "groups",
  ],
  "nativeThreadProjection.receiversPerCollaborationItem": [
    "persistCollaboration",
    "receivers",
  ],
  "nativeThreadProjection.activityNodesPerPage": [
    "readActivityPage",
    "requestedNodes",
  ],
  "nativeThreadProjection.activityLabelBytes": [
    "persistActivity",
    "labelBytes",
  ],
  "nativeThreadProjection.activityMessageBytes": [
    "persistActivity",
    "messageBytes",
  ],
};

const derivedMeasurements = {
  "wire.metadataBytes": (steps) => {
    const step = one(steps, "emitMetadata");
    return step.aggregateBytes ?? step.entryBytes.reduce((a, b) => a + b, 0);
  },
  "wire.modelsOrConfigBytes": (steps) => {
    const step = one(steps, "emitModels");
    return (
      step.repeatedEntry.idBytes * step.repeatedEntry.count +
      step.repeatedEntry.nameBytes * step.repeatedEntry.count +
      (step.entryBytes ?? []).reduce((a, b) => a + b, 0)
    );
  },
  "wire.itemBytesPerTurn": (steps) => {
    const step = one(steps, "persistItems");
    return (
      step.repeatedItemBytes.bytes * step.repeatedItemBytes.count +
      (step.additionalItemBytes ?? []).reduce((a, b) => a + b, 0)
    );
  },
  "queues.driverDataBytes": (steps) => {
    const step = one(steps, "fillDriverData");
    return (
      step.repeatedMessageBytes.bytes * step.repeatedMessageBytes.count +
      (step.additionalMessageBytes ?? []).reduce((a, b) => a + b, 0)
    );
  },
  "queues.egressFrames": (steps) => sum(steps, "frames"),
  "queues.egressBytes": (steps) => sum(steps, "bytes"),
  "queues.journalBatches": (steps) => sum(steps, "batches"),
  "queues.journalBytes": (steps) => sum(steps, "bytes"),
  "process.argumentBytesEach": (steps) =>
    Math.max(...one(steps, "launchShape").argumentBytes),
  "process.argumentBytesTotal": (steps) =>
    one(steps, "launchShape").argumentBytes.reduce((a, b) => a + b, 0),
  "process.environmentBytesTotal": (steps) =>
    one(steps, "launchShape").environmentPairBytes.reduce((a, b) => a + b, 0),
  "process.windowsJobActiveProcessLimit": (steps) => {
    const step = one(steps, "spawnTree");
    return step.roots + step.descendants;
  },
  "process.descendantsPerTree": (steps) => {
    const step = steps.find(
      (candidate) => candidate.op === "spawnFixtureChildren",
    );
    return step?.count ?? Number.NaN;
  },
  "process.stderrRingBytes": (steps) => {
    const step = one(steps, "fillStderrRing");
    return (
      (step.repeatedLineBytes.bytes + 1) * step.repeatedLineBytes.count +
      ("finalLineBytes" in step ? step.finalLineBytes + 1 : 0)
    );
  },
  "nativeThreadProjection.observedDepth": (steps) => {
    const shape = one(steps, "emitKnownFrame").shape;
    const match = /^childActivityDepth([0-9]+)$/u.exec(shape);
    return match ? Number(match[1]) : Number.NaN;
  },
};

const measuredPaths = new Set([
  ...Object.keys(directMeasurements),
  ...Object.keys(derivedMeasurements),
]);
const coveredPaths = new Set(limitCoverage.pairs.map(([path]) => path));
if (
  measuredPaths.size !== limitCoverage.pairs.length ||
  measuredPaths.size !== coveredPaths.size ||
  [...measuredPaths].some((path) => !coveredPaths.has(path))
) {
  throw new Error("numeric fixture path mapping must be closed and exact");
}
for (const [path, exactId, plusOneId] of limitCoverage.pairs) {
  const exactCase = casesById.get(exactId);
  const plusOneCase = casesById.get(plusOneId);
  if (!exactCase || !plusOneCase) {
    throw new Error(
      `numeric fixture pair references a missing case for ${path}`,
    );
  }
  const direct = directMeasurements[path];
  const measure = direct
    ? (steps) => one(steps, direct[0])[direct[1]]
    : derivedMeasurements[path];
  const limit = limitAt(path);
  if (
    measure(exactCase.steps) !== limit ||
    measure(plusOneCase.steps) !== limit + 1
  ) {
    throw new Error(`numeric fixture pair does not prove ${path} exact/+1`);
  }
}

const expectedOwnership = execution.requiredOwnershipCounts;
const expectedPhases = execution.requiredPhaseCounts;
if (
  ownershipCounts.Child !== expectedOwnership.child ||
  ownershipCounts.Host !== expectedOwnership.host ||
  phaseCounts.Foundation !== expectedPhases.headlessFoundation ||
  phaseCounts.Windows !== expectedPhases.windows ||
  phaseCounts.Package !== expectedPhases.package ||
  phaseCounts.Deferred !== expectedPhases.deferredFoundation
) {
  throw new Error(
    "fixture execution ownership or phase counts do not match the frozen contract",
  );
}

const sha256 = createHash("sha256").update(source).digest("hex");
const generated = `// @generated by scripts/generate-acp-fixture-cases.mjs
// frozen fixture-vectors-v1.json SHA-256 ${sha256}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FrozenOwner {
    Child,
    Host,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FrozenPhase {
    Foundation,
    Windows,
    Package,
    Deferred,
}

pub(super) struct FrozenCase {
    pub(super) id: &'static str,
    pub(super) steps_json: &'static str,
    pub(super) owner: FrozenOwner,
    pub(super) phase: FrozenPhase,
    #[cfg(test)]
    pub(super) oracle_json: &'static str,
}

#[rustfmt::skip]
pub(super) const FROZEN_CASES: &[FrozenCase] = &[
${rows.join("\n")}
];

pub(super) struct JourneyCase {
    pub(super) id: &'static str,
    pub(super) steps_json: &'static str,
}

#[rustfmt::skip]
pub(super) const JOURNEY_CASES: &[JourneyCase] = &[
${journeyRows.join("\n")}
];
`;

if (process.argv.includes("--check")) {
  if (readFileSync(outputPath, "utf8") !== generated) {
    throw new Error("generated ACP fixture table is stale");
  }
  console.log("ACP fixture contract and numeric exact/+1 mapping are current");
} else {
  writeFileSync(outputPath, generated);
}
