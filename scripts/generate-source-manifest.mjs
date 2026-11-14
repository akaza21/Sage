import { createHash } from "node:crypto";
import {
  mkdirSync,
  lstatSync,
  readFileSync,
  readdirSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptPath = fileURLToPath(import.meta.url);
const workspaceRoot = path.resolve(path.dirname(scriptPath), "..");
const policyVersion = 1;

export const defaultPolicy = {
  files: [
    ".editorconfig",
    ".gitattributes",
    ".gitignore",
    ".prettierignore",
    "Cargo.lock",
    "Cargo.toml",
    "deny.toml",
    "eslint.config.js",
    "index.html",
    "package-lock.json",
    "package.json",
    "rust-toolchain.toml",
    "tsconfig.json",
    "vite.config.ts",
  ],
  directories: [
    "crates",
    "scripts",
    "src",
    "src-tauri",
    "tools/footprint-control",
  ],
  excludedSegments: [
    ".git",
    "dist",
    "gen",
    "local-evidence",
    "node_modules",
    "target",
  ],
  bounds: {
    files: 4096,
    fileBytes: 16777216,
    sourceBytes: 134217728,
    manifestBytes: 8388608,
    directoryDepth: 32,
  },
};

function effectivePolicy(overrides) {
  return {
    ...defaultPolicy,
    ...overrides,
    bounds: {
      ...defaultPolicy.bounds,
      ...overrides?.bounds,
    },
  };
}

function codePointCompare(left, right) {
  const leftPoints = [...left].map((value) => value.codePointAt(0));
  const rightPoints = [...right].map((value) => value.codePointAt(0));
  const count = Math.min(leftPoints.length, rightPoints.length);
  for (let index = 0; index < count; index += 1) {
    if (leftPoints[index] !== rightPoints[index]) {
      return leftPoints[index] - rightPoints[index];
    }
  }
  return leftPoints.length - rightPoints.length;
}

function normalizedRelative(root, absolutePath) {
  const relative = path.relative(root, absolutePath).split(path.sep).join("/");
  if (
    !relative ||
    relative.startsWith("../") ||
    relative.includes("\\") ||
    relative.includes("\0")
  ) {
    throw new Error(`manifest path is not canonical: ${relative}`);
  }
  return relative;
}

function collectDirectory(root, relativeDirectory, policy, output, depth = 1) {
  if (depth > policy.bounds.directoryDepth) {
    throw new Error(
      `manifest directory depth exceeds its bound: ${relativeDirectory}`,
    );
  }
  const absolute = path.join(root, relativeDirectory);
  for (const entry of readdirSync(absolute, { withFileTypes: true })) {
    if (policy.excludedSegments.includes(entry.name)) continue;
    const child = path.join(absolute, entry.name);
    const relative = normalizedRelative(root, child);
    if (entry.isDirectory()) {
      collectDirectory(root, relative, policy, output, depth + 1);
    } else if (entry.isFile()) {
      if (output.length >= policy.bounds.files) {
        throw new Error("manifest file count exceeds its bound");
      }
      output.push(relative);
    } else {
      throw new Error(`manifest input must be a regular file: ${relative}`);
    }
  }
}

export function buildSourceManifest(root, overrides = defaultPolicy) {
  const policy = effectivePolicy(overrides);
  const paths = [];
  for (const relative of policy.files) {
    const absolute = path.join(root, relative);
    if (!lstatSync(absolute).isFile()) {
      throw new Error(`required manifest input is not a file: ${relative}`);
    }
    if (paths.length >= policy.bounds.files) {
      throw new Error("manifest file count exceeds its bound");
    }
    paths.push(normalizedRelative(root, absolute));
  }
  for (const directory of policy.directories) {
    collectDirectory(root, directory, policy, paths);
  }
  paths.sort(codePointCompare);
  const files = paths.map((relativePath) => {
    const absolutePath = path.join(root, relativePath);
    const item = lstatSync(absolutePath);
    if (!item.isFile() || item.size > policy.bounds.fileBytes) {
      throw new Error(`manifest input exceeds its file bound: ${relativePath}`);
    }
    const bytes = readFileSync(absolutePath);
    return {
      path: relativePath,
      bytes: bytes.byteLength,
      sha256: createHash("sha256").update(bytes).digest("hex"),
    };
  });
  const aggregate = createHash("sha256");
  for (const file of files) {
    aggregate.update(file.path, "utf8");
    aggregate.update("\0");
    aggregate.update(file.sha256, "utf8");
    aggregate.update("\0");
    aggregate.update(String(file.bytes), "utf8");
    aggregate.update("\n");
  }
  const totalBytes = files.reduce((total, file) => total + file.bytes, 0);
  if (totalBytes > policy.bounds.sourceBytes) {
    throw new Error("manifest source bytes exceed their bound");
  }
  const manifest = {
    schemaVersion: 1,
    policyVersion,
    pathFormat: "workspace-relative UTF-8 with slash separators",
    ordering: "Unicode code point",
    aggregateConstruction: "SHA-256(path NUL sha256 NUL byteLength LF)",
    include: policy,
    nodeVersion: process.version,
    generatorSha256: createHash("sha256")
      .update(readFileSync(scriptPath))
      .digest("hex"),
    aggregateSha256: aggregate.digest("hex"),
    fileCount: files.length,
    totalBytes,
    files,
  };
  if (
    Buffer.byteLength(JSON.stringify(manifest)) > policy.bounds.manifestBytes
  ) {
    throw new Error("serialized source manifest exceeds its bound");
  }
  return manifest;
}

const defaultCandidateOptions = {
  workItem: "ADE-0005",
  rawManifest:
    "%WORKSPACE%/local-evidence/ADE-0005-project-lifecycle/fix-cycle-1/source-manifest.json",
};

export function curatedCandidate(manifest, options = defaultCandidateOptions) {
  const { workItem, rawManifest } = {
    ...defaultCandidateOptions,
    ...options,
  };
  if (!/^ADE-[0-9]{4}[A-Z]?$/.test(workItem)) {
    throw new Error(`invalid work item: ${workItem}`);
  }
  if (
    !rawManifest.startsWith("%WORKSPACE%/local-evidence/") ||
    !rawManifest.endsWith(".json") ||
    rawManifest.includes("..") ||
    rawManifest.includes("\\") ||
    rawManifest.includes("\0")
  ) {
    throw new Error(`invalid raw manifest label: ${rawManifest}`);
  }
  return {
    schemaVersion: manifest.schemaVersion,
    workItem,
    sanitized: true,
    policyVersion: manifest.policyVersion,
    aggregateSha256: manifest.aggregateSha256,
    fileCount: manifest.fileCount,
    totalBytes: manifest.totalBytes,
    generatorSha256: manifest.generatorSha256,
    nodeVersion: manifest.nodeVersion,
    pathFormat: manifest.pathFormat,
    ordering: manifest.ordering,
    aggregateConstruction: manifest.aggregateConstruction,
    rawManifest,
  };
}

export function validateEvidenceBinding(candidate, evidence) {
  const errors = [];
  if (evidence.candidate?.aggregateSha256 !== candidate.aggregateSha256) {
    errors.push(
      "evidence candidate aggregate does not match the source manifest",
    );
  }
  const executable = evidence.artifacts?.final?.executableSha256;
  if (evidence.candidate?.executableSha256 !== executable) {
    errors.push("candidate executable does not match the final artifact");
  }
  for (const campaign of evidence.candidate?.campaigns ?? []) {
    if (campaign.executableSha256 !== executable) {
      errors.push(`${campaign.name} uses a stale executable`);
    }
    if (campaign.aggregateSha256 !== candidate.aggregateSha256) {
      errors.push(`${campaign.name} uses a stale source candidate`);
    }
  }
  return errors;
}

function argument(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function rawManifestLabel(output) {
  const relative = normalizedRelative(workspaceRoot, path.resolve(output));
  if (!relative.startsWith("local-evidence/")) {
    throw new Error("raw source manifest must be stored under local-evidence");
  }
  return `%WORKSPACE%/${relative}`;
}

if (process.argv[1] && path.resolve(process.argv[1]) === scriptPath) {
  const rawOutput = argument("--output");
  const curatedOutput = argument("--curated-output");
  const workItem = argument("--work-item") ?? defaultCandidateOptions.workItem;
  if (!rawOutput || !curatedOutput) {
    throw new Error(
      "usage: generate-source-manifest.mjs --output <ignored-json> --curated-output <json> [--work-item ADE-0000]",
    );
  }
  const manifest = buildSourceManifest(workspaceRoot);
  manifest.command = `node scripts/generate-source-manifest.mjs --output %RAW_MANIFEST% --curated-output %CURATED_MANIFEST% --work-item ${workItem}`;
  for (const output of [rawOutput, curatedOutput]) {
    mkdirSync(path.dirname(path.resolve(output)), { recursive: true });
  }
  writeFileSync(rawOutput, `${JSON.stringify(manifest, null, 2)}\n`);
  writeFileSync(
    curatedOutput,
    `${JSON.stringify(
      curatedCandidate(manifest, {
        workItem,
        rawManifest: rawManifestLabel(rawOutput),
      }),
      null,
      2,
    )}\n`,
  );
  console.log(
    JSON.stringify({
      status: "passed",
      aggregateSha256: manifest.aggregateSha256,
      fileCount: manifest.fileCount,
      totalBytes: manifest.totalBytes,
    }),
  );
}
