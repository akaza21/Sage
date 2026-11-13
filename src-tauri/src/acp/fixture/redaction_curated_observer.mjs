import assert from "node:assert/strict";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  exactKeys,
  inspectSanitized,
  parseBoundedJson,
} from "../../../../scripts/ade0006a-c0b-common.mjs";
import { loadFrozenContracts } from "../../../../scripts/ade0006a-c0b-contracts.mjs";

const scriptPath = fileURLToPath(import.meta.url);

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
  const frozen = loadFrozenContracts();
  const fixtureCase = frozen.fixture.cases.find(
    (candidate) => candidate.id === "redaction-canaries-all-sinks",
  );
  assert.ok(fixtureCase);
  const canaries = fixtureCase.steps[0].canaries;
  const curated = parseBoundedJson(
    inputPath,
    frozen.evidence.campaignBounds.curatedBytes,
    frozen.limits.wire,
    "redaction curated evidence",
  ).value;
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
