import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const repositoryRoot = process.cwd();
const contractPath = path.join(
  repositoryRoot,
  "docs",
  "work-items",
  "BETA-0001-primary-beta-path",
  "contracts",
  "beta-acceptance-v1.json",
);

async function contract() {
  return JSON.parse(await readFile(contractPath, "utf8"));
}

test("beta contract freezes single-root Codex multi-Chat durability", async () => {
  const value = await contract();

  assert.equal(value.status, "frozen-thresholds-awaiting-candidate-verifier");
  assert.equal(value.codexJourney.projectRootMode, "single-root-project-only");
  assert.equal(value.codexJourney.minimumDistinctChats, 2);
  assert.equal(value.codexJourney.minimumSequentialTurnsInOneSession, 2);
  assert.equal(value.codexJourney.minimumSuccessfulTurns, 4);
  assert.equal(value.codexJourney.minimumRestartResumeThenTurnCycles, 1);
  assert.equal(value.codexJourney.minimumCancelThenReuseCycles, 1);
  assert.equal(value.codexJourney.minimumCleanCycles, 5);
  assert.equal(value.codexJourney.minimumFailureCycles, 5);
  assert.equal(value.codexJourney.maximumOrphanProcessesAfterClose, 0);
});

test("beta contract binds executable Codex ceilings and acceptance owners", async () => {
  const value = await contract();

  assert.equal(value.codexBudgets.minimumLatencySamples, 30);
  assert.equal(value.codexBudgets.operationalRuntimeTreesMaximum, 1);
  assert.equal(value.codexBudgets.operationalProcessCountMaximum, 1);
  assert.equal(value.codexBudgets.managedStateLogicalBytesMaximum, 536_870_912);
  assert.equal(value.codexBudgets.readyIdleCpuSingleCorePercentP95Maximum, 0.5);
  assert.equal(value.codexBudgets.cancelDispatchP95MsMaximum, 100);
  assert.equal(value.codexBudgets.cancelTerminalP95MsMaximum, 1_000);
  assert.equal(value.codexBudgets.explicitCloseP95MsMaximum, 1_000);
  assert.equal(value.candidate.installedCampaignRequired, true);
  assert.equal(value.candidate.freshStandardsReviewRequired, true);
  assert.equal(value.candidate.freshSpecReviewRequired, true);
  assert.equal(value.candidate.independentTestRequired, true);

  assert.equal(value.candidateVerifier.status, "planned");
  assert.equal(
    value.candidateVerifier.requiredBeforeFeatureFreezeCampaign,
    true,
  );

  for (const authority of value.sharedAuthorities) {
    const bytes = await readFile(
      path.resolve(path.dirname(contractPath), authority.relativePath),
    );
    assert.equal(
      bytes.length,
      authority.bytes,
      `${authority.id} byte identity`,
    );
    assert.equal(
      createHash("sha256").update(bytes).digest("hex"),
      authority.sha256,
      `${authority.id} SHA-256 identity`,
    );
  }
});
