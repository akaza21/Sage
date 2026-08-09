import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  exactKeys,
  parseBoundedJson,
  readBoundedFile,
  sha256,
} from "./ade0006a-c0b-common.mjs";
import { loadAuthenticatedFrontendBaseline } from "./ade0006a-package-binding.mjs";

const scriptRoot = path.dirname(fileURLToPath(import.meta.url));
export const defaultWorkspaceRoot = path.resolve(scriptRoot, "..");
const contractRelative =
  "docs/work-items/ADE-0006A-acp-session-foundation/contracts";
const bootstrapWire = {
  jsonDepth: 32,
  jsonNodes: 16384,
  objectMembers: 256,
  arrayEntries: 512,
  decodedKeyBytes: 256,
};

function manifestEntry(manifest, relativePath) {
  return manifest.files.find((entry) => entry.relativePath === relativePath);
}

function validateManifest(manifest, directory) {
  const errors = [];
  exactKeys(
    errors,
    manifest,
    [
      "schemaVersion",
      "status",
      "hashAlgorithm",
      "selfIncluded",
      "ordering",
      "fileCount",
      "logicalBytes",
      "files",
    ],
    "contractManifest",
  );
  if (
    manifest.schemaVersion !== 1 ||
    manifest.hashAlgorithm !== "sha256" ||
    manifest.selfIncluded !== false ||
    !Array.isArray(manifest.files) ||
    manifest.files.length !== manifest.fileCount
  ) {
    errors.push("contractManifest header is invalid");
    return errors;
  }
  const names = manifest.files.map(({ relativePath }) => relativePath);
  if (
    new Set(names).size !== names.length ||
    [...names]
      .sort((left, right) => Buffer.from(left).compare(Buffer.from(right)))
      .join("\0") !== names.join("\0")
  ) {
    errors.push("contractManifest paths are duplicate or out of order");
  }
  let logicalBytes = 0;
  for (const entry of manifest.files) {
    exactKeys(
      errors,
      entry,
      ["relativePath", "bytes", "sha256"],
      `contractManifest.${entry.relativePath}`,
    );
    if (
      typeof entry.relativePath !== "string" ||
      !/^[a-z0-9][a-z0-9./-]*$/i.test(entry.relativePath) ||
      entry.relativePath.includes("..") ||
      !Number.isSafeInteger(entry.bytes) ||
      entry.bytes < 1 ||
      entry.bytes > 64 * 1024 * 1024
    ) {
      errors.push(`contractManifest.${entry.relativePath} metadata is invalid`);
      continue;
    }
    const bytes = readBoundedFile(
      path.join(directory, entry.relativePath),
      entry.bytes,
      `contract ${entry.relativePath}`,
    );
    if (bytes.byteLength !== entry.bytes || sha256(bytes) !== entry.sha256) {
      errors.push(`contract ${entry.relativePath} does not match its manifest`);
    }
    logicalBytes += bytes.byteLength;
  }
  if (logicalBytes !== manifest.logicalBytes) {
    errors.push("contractManifest logicalBytes does not match its files");
  }
  return errors;
}

function readManifestMember(directory, manifest, filename, wire) {
  const entry = manifestEntry(manifest, filename);
  if (!entry) throw new Error(`contract manifest omits ${filename}`);
  const record = parseBoundedJson(
    path.join(directory, filename),
    entry.bytes,
    wire,
    `contract ${filename}`,
  );
  if (record.bytes !== entry.bytes || record.sha256 !== entry.sha256) {
    throw new Error(`contract ${filename} changed after manifest verification`);
  }
  return record.value;
}

export function deriveQueueBoundaries(limits) {
  const queues = limits.queues;
  const durable = limits.durable;
  return new Map([
    [
      "driverDataMessages",
      {
        kind: "count",
        count: queues.driverDataMessages,
        bytes: queues.driverDataBytes,
      },
    ],
    [
      "driverDataBytes",
      {
        kind: "bytes",
        count: queues.driverDataMessages,
        bytes: queues.driverDataBytes,
      },
    ],
    [
      "driverControlMessages",
      {
        kind: "count",
        count: queues.driverControlMessages,
        bytes: queues.driverControlBytes,
      },
    ],
    [
      "driverControlBytes",
      {
        kind: "bytes",
        count: queues.driverControlMessages,
        bytes: queues.driverControlBytes,
      },
    ],
    [
      "egressDataFrames",
      {
        kind: "count",
        count: queues.egressFrames - queues.egressControlFramesInsideTotal,
        bytes: queues.egressBytes - queues.egressControlBytesInsideTotal,
      },
    ],
    [
      "egressDataBytes",
      {
        kind: "bytes",
        count: queues.egressFrames - queues.egressControlFramesInsideTotal,
        bytes: queues.egressBytes - queues.egressControlBytesInsideTotal,
      },
    ],
    [
      "egressControlFrames",
      {
        kind: "count",
        count: queues.egressControlFramesInsideTotal,
        bytes: queues.egressControlBytesInsideTotal,
      },
    ],
    [
      "egressControlBytes",
      {
        kind: "bytes",
        count: queues.egressControlFramesInsideTotal,
        bytes: queues.egressControlBytesInsideTotal,
      },
    ],
    [
      "journalDataBatches",
      {
        kind: "count",
        count: queues.journalBatches - queues.journalControlBatchesInsideTotal,
        bytes: queues.journalBytes - queues.journalControlBytesInsideTotal,
      },
    ],
    [
      "journalDataBytes",
      {
        kind: "bytes",
        count: queues.journalBatches - queues.journalControlBatchesInsideTotal,
        bytes: queues.journalBytes - queues.journalControlBytesInsideTotal,
      },
    ],
    [
      "journalControlBatches",
      {
        kind: "count",
        count: queues.journalControlBatchesInsideTotal,
        bytes: queues.journalControlBytesInsideTotal,
      },
    ],
    [
      "journalControlBytes",
      {
        kind: "bytes",
        count: queues.journalControlBatchesInsideTotal,
        bytes: queues.journalControlBytesInsideTotal,
      },
    ],
    [
      "pageReadsPerOwner",
      {
        kind: "count",
        count: durable.outstandingPageReadsPerSession,
        bytes:
          durable.historyPageBytes * durable.outstandingPageReadsPerSession,
      },
    ],
    [
      "pageReadsGlobal",
      {
        kind: "count",
        count: durable.outstandingPageReadsGlobal,
        bytes: durable.historyPageBytes * durable.outstandingPageReadsGlobal,
      },
    ],
  ]);
}

export function loadFrozenContracts(root = defaultWorkspaceRoot) {
  const directory = path.join(root, contractRelative);
  const manifestRecord = parseBoundedJson(
    path.join(directory, "contract-manifest-v1.json"),
    256 * 1024,
    bootstrapWire,
    "contract manifest",
  );
  const errors = validateManifest(manifestRecord.value, directory);
  if (errors.length > 0) throw new Error(errors.join("\n"));
  const rawLimits = readManifestMember(
    directory,
    manifestRecord.value,
    "limits-v1.json",
    bootstrapWire,
  );
  const wire = rawLimits.wire;
  const frozen = {
    manifest: manifestRecord.value,
    contractManifestSha256: manifestRecord.sha256,
    limits: rawLimits,
    budgets: readManifestMember(
      directory,
      manifestRecord.value,
      "performance-budgets-v1.json",
      wire,
    ),
    package: readManifestMember(
      directory,
      manifestRecord.value,
      "package-contract-v1.json",
      wire,
    ),
    gestures: readManifestMember(
      directory,
      manifestRecord.value,
      "packaged-gestures-v1.json",
      wire,
    ),
    evidence: readManifestMember(
      directory,
      manifestRecord.value,
      "evidence-policy-v1.json",
      wire,
    ),
    fixture: readManifestMember(
      directory,
      manifestRecord.value,
      "fixture-vectors-v1.json",
      wire,
    ),
  };
  frozen.fixtureManifestSha256 = manifestEntry(
    frozen.manifest,
    "fixture-vectors-v1.json",
  ).sha256;
  frozen.packagedGesturesSha256 = manifestEntry(
    frozen.manifest,
    "packaged-gestures-v1.json",
  ).sha256;
  frozen.packageContractSha256 = manifestEntry(
    frozen.manifest,
    "package-contract-v1.json",
  ).sha256;
  frozen.limitsSha256 = manifestEntry(frozen.manifest, "limits-v1.json").sha256;
  frozen.queueBoundaries = deriveQueueBoundaries(frozen.limits);
  frozen.frontendBaseline = loadAuthenticatedFrontendBaseline({
    sourceRoot: root,
    frozen,
  });
  return frozen;
}
