import assert from "node:assert/strict";
import { lstatSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptPath = fileURLToPath(import.meta.url);
const contracts = path.resolve(
  path.dirname(scriptPath),
  "../../../testdata/acp/contracts",
);

function contract(name) {
  return JSON.parse(readFileSync(path.join(contracts, name), "utf8"));
}

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function readBoundedJson(filePath, maximumBytes, limits, label) {
  const metadata = lstatSync(filePath);
  if (!metadata.isFile() || metadata.isSymbolicLink()) {
    throw new Error(`${label} must be a regular non-symlink file`);
  }
  if (metadata.size < 1 || metadata.size > maximumBytes) {
    throw new Error(`${label} must contain 1..${maximumBytes} bytes`);
  }
  const value = JSON.parse(readFileSync(filePath, "utf8"));
  const stack = [{ value, depth: 1 }];
  let nodes = 0;
  while (stack.length > 0) {
    const current = stack.pop();
    nodes += 1;
    if (nodes > limits.jsonNodes) {
      throw new Error(`${label} exceeds ${limits.jsonNodes} JSON nodes`);
    }
    if (current.depth > limits.jsonDepth) {
      throw new Error(`${label} exceeds JSON depth ${limits.jsonDepth}`);
    }
    if (Array.isArray(current.value)) {
      if (current.value.length > limits.arrayEntries) {
        throw new Error(
          `${label} array exceeds ${limits.arrayEntries} entries`,
        );
      }
      for (const entry of current.value) {
        stack.push({ value: entry, depth: current.depth + 1 });
      }
    } else if (isRecord(current.value)) {
      const entries = Object.entries(current.value);
      if (entries.length > limits.objectMembers) {
        throw new Error(
          `${label} object exceeds ${limits.objectMembers} members`,
        );
      }
      for (const [key, entry] of entries) {
        if (Buffer.byteLength(key) > limits.decodedKeyBytes) {
          throw new Error(
            `${label} key exceeds ${limits.decodedKeyBytes} bytes`,
          );
        }
        stack.push({ value: entry, depth: current.depth + 1 });
      }
    }
  }
  return value;
}

function exactKeys(errors, value, keys, label) {
  if (!isRecord(value)) {
    errors.push(`${label} must be an object`);
    return;
  }
  const actual = Object.keys(value).sort().join("\0");
  const expected = [...keys].sort().join("\0");
  if (actual !== expected)
    errors.push(`${label} has missing or unknown fields`);
}

const unsafeKey =
  /(^|_)(argv|environment|workingdirectory|absolutepath|secret|token|commandline|rawframe|rawpayload)(_|$)/i;
const thresholdKey = /(budget|ceiling|threshold)/i;
const embeddedPath =
  /(?:file:(?:\/{2,3}|\\\\)|(?:^|[\s="'([{])(?:[a-z]:[\\/]|\\\?\?\\[a-z]:[\\/]|\\\\(?:\?\\|\.\\|[^\\])|\\[a-z0-9._-]+(?:[\\/][a-z0-9._-]+)*(?=$|[\s="')\]}])|\/[a-z0-9._-]+(?:\/[a-z0-9._-]+)*(?=$|[\s="')\]}])))/i;
const embeddedSecret =
  /(?:bearer\s+[a-z0-9._-]+|(?:api[-_ ]?key|password|secret|token)\s*[=:]\s*\S+)/i;
const embeddedCommand =
  /(?:^|\s)(?:(?:cmd|powershell|bash|zsh|node|npm|cargo|git|python(?:3(?:\.\d+)?)?)(?:\.exe|\.cmd)?\s+(?:--?|[./\\]|[a-z0-9_<])|sage(?:\.exe)?\s+(?:--?|[./\\]))/i;

// Retained evidence may carry no path, secret, command line, raw payload or
// self-declared threshold anywhere in its tree.
function inspectSanitized(value, errors, label) {
  const stack = [{ value, label }];
  while (stack.length > 0) {
    const current = stack.pop();
    if (typeof current.value === "string") {
      if (Buffer.byteLength(current.value) > 8192) {
        errors.push(`${current.label} exceeds 8192 sanitized bytes`);
      }
      if (embeddedPath.test(current.value)) {
        errors.push(`${current.label} contains an embedded absolute path`);
      }
      if (embeddedSecret.test(current.value)) {
        errors.push(`${current.label} contains secret-like material`);
      }
      if (embeddedCommand.test(current.value)) {
        errors.push(`${current.label} contains a command line`);
      }
    } else if (Array.isArray(current.value)) {
      current.value.forEach((entry, index) =>
        stack.push({ value: entry, label: `${current.label}[${index}]` }),
      );
    } else if (isRecord(current.value)) {
      for (const [key, entry] of Object.entries(current.value)) {
        if (unsafeKey.test(key))
          errors.push(`${current.label}.${key} is forbidden`);
        if (thresholdKey.test(key)) {
          errors.push(`${current.label}.${key} defines an untrusted threshold`);
        }
        stack.push({ value: entry, label: `${current.label}.${key}` });
      }
    }
  }
}

function argument(name) {
  const index = process.argv.indexOf(name);
  return index < 0 ? undefined : process.argv[index + 1];
}

function occurrences(haystack, needle) {
  let count = 0;
  let offset = 0;
  while ((offset = haystack.indexOf(needle, offset)) >= 0) {
    count += 1;
    offset += needle.length;
  }
  return count;
}

export function observeCuratedRedaction(inputPath) {
  const limits = contract("limits-v1.json");
  const fixtureCase = contract("fixture-vectors-v1.json").cases.find(
    (candidate) => candidate.id === "redaction-canaries-all-sinks",
  );
  assert.ok(fixtureCase);
  const canaries = fixtureCase.steps[0].canaries;
  const curated = readBoundedJson(
    inputPath,
    limits.fixture.curatedEvidenceBytes,
    limits.wire,
    "redaction curated evidence",
  );
  const sanitizationErrors = [];
  exactKeys(
    sanitizationErrors,
    curated,
    ["projection", "sqlite", "stderr", "structuredLog", "typedRedactions"],
    "redactionEvidence",
  );
  inspectSanitized(curated, sanitizationErrors, "redactionEvidence");
  assert.deepEqual(sanitizationErrors, []);
  assert.deepEqual(
    curated.typedRedactions.map((redaction) => redaction.source),
    canaries.map((canary, caseIndex) => ({
      caseIndex,
      sink: canary.split(".")[1],
    })),
  );
  assert.equal(
    curated.typedRedactions.filter(
      (redaction) => redaction.source.sink === "curatedEvidence",
    ).length,
    5,
  );

  const retained = JSON.stringify(curated);
  const stderr = JSON.stringify(curated.stderr);
  const observedOracle = {
    expectedOccurrencesByCanary: canaries.map((canary) =>
      occurrences(retained, canary),
    ),
    typedRedactionCount: curated.typedRedactions.length,
    unredactedStderr: canaries.some((canary) => stderr.includes(canary)),
  };
  assert.deepEqual(observedOracle, fixtureCase.oracle);
  return observedOracle;
}

if (process.argv[1] && path.resolve(process.argv[1]) === scriptPath) {
  const inputPath = argument("--input");
  if (!inputPath) {
    console.error(
      "usage: node src-tauri/src/acp/fixture/redaction_curated_observer.mjs --input <curated.json>",
    );
    process.exitCode = 1;
  } else {
    try {
      process.stdout.write(
        `${JSON.stringify(observeCuratedRedaction(inputPath))}\n`,
        (error) => {
          if (!error) process.stderr.write("sage-redaction:sink-emitted\n");
        },
      );
    } catch (error) {
      console.error(`redaction-observer: ${error.message}`);
      process.exitCode = 1;
    }
  }
}
