import assert from "node:assert/strict";
import test from "node:test";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  SOURCE_FUNCTIONAL_PROFILE,
  SOURCE_FUNCTIONAL_REQUIRED_IDS,
  currentWorkingTreeResult,
  evaluateSourceFunctionalEvidence,
} from "./beta0001-source-functional-profile.mjs";

const digest = (value) => createHash("sha256").update(value).digest("hex");
const evidenceRelative = "local-evidence/BETA-0001/source-functional";
const manifestRelative = `${evidenceRelative}/attestation-v1.json`;

function git(root, args) {
  return execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();
}

function commit(root, message) {
  git(root, ["add", "."]);
  git(root, [
    "-c",
    "user.name=test",
    "-c",
    "user.email=test@example.invalid",
    "commit",
    "-m",
    message,
  ]);
  return git(root, ["rev-parse", "HEAD"]);
}

function writeAttestation(fixture, inventory) {
  writeFileSync(
    path.join(fixture.root, manifestRelative),
    `${JSON.stringify({
      candidateCommit: fixture.candidateCommit,
      candidateTree: fixture.candidateTree,
      inventory,
    })}\n`,
  );
}

function attestedFixture({ statuses } = {}) {
  const root = mkdtempSync(path.join(tmpdir(), "sage-source-evidence-"));
  execFileSync("git", ["init"], { cwd: root });
  writeFileSync(path.join(root, "pinned.txt"), "immutable\n");
  const candidateCommit = commit(root, "candidate");
  const candidateTree = git(root, ["rev-parse", "HEAD^{tree}"]);
  const evidenceRoot = path.join(root, evidenceRelative);
  mkdirSync(evidenceRoot, { recursive: true });
  const inventory = SOURCE_FUNCTIONAL_REQUIRED_IDS.map((id, index) => {
    const file = `evidence/${id}.json`;
    const bytes = Buffer.from(id);
    const evidenceFile = path.join(evidenceRoot, file);
    mkdirSync(path.dirname(evidenceFile), { recursive: true });
    writeFileSync(evidenceFile, bytes);
    return {
      id,
      status: statuses?.[index] ?? "passed",
      path: file,
      sha256: digest(bytes),
    };
  });
  const fixture = {
    root,
    candidateCommit,
    candidateTree,
    evidenceRoot,
    inventory,
  };
  writeAttestation(fixture, inventory);
  fixture.attestationCommit = commit(root, "attestation");
  return fixture;
}

test("source-functional profile explicitly defers installed package gates", () => {
  assert.equal(SOURCE_FUNCTIONAL_PROFILE.workItem, "BETA-0001");
  assert.deepEqual(SOURCE_FUNCTIONAL_PROFILE.deferredEvidence, [
    "windows.source-package",
    "windows.installed-lifecycle",
  ]);
  assert.ok(!SOURCE_FUNCTIONAL_REQUIRED_IDS.includes("windows.source-package"));
  assert.ok(
    !SOURCE_FUNCTIONAL_REQUIRED_IDS.includes("windows.installed-lifecycle"),
  );
  assert.equal(SOURCE_FUNCTIONAL_PROFILE.acceptance, "local-green-only");
});

test("attests a candidate commit through a later committed manifest", () => {
  const fixture = attestedFixture();
  assert.equal(
    evaluateSourceFunctionalEvidence({
      sourceCommit: fixture.candidateCommit,
      attestationCommit: fixture.attestationCommit,
      repositoryRoot: fixture.root,
    }).verdict,
    "passed",
  );
});

test("current evaluation consumes an explicit later attestation commit", () => {
  const fixture = attestedFixture();
  assert.equal(
    currentWorkingTreeResult({
      repositoryRoot: fixture.root,
      attestationCommit: fixture.attestationCommit,
    }).verdict,
    "passed",
  );
});

test("does not treat caller evidence descriptors as authority", () => {
  const fixture = attestedFixture();
  assert.equal(
    evaluateSourceFunctionalEvidence({
      sourceCommit: fixture.candidateCommit,
      attestationCommit: fixture.attestationCommit,
      evidence: SOURCE_FUNCTIONAL_REQUIRED_IDS.map((id) => ({
        id,
        status: "failed",
      })),
      repositoryRoot: fixture.root,
    }).verdict,
    "passed",
  );
});

test("rejects a stale candidate commit", () => {
  const fixture = attestedFixture();
  assert.throws(
    () =>
      evaluateSourceFunctionalEvidence({
        sourceCommit: fixture.attestationCommit,
        attestationCommit: fixture.attestationCommit,
        repositoryRoot: fixture.root,
      }),
    /source-functional-binding-stale/u,
  );
});

test("does not let HEAD attest itself", () => {
  const fixture = attestedFixture();
  assert.throws(
    () =>
      evaluateSourceFunctionalEvidence({
        sourceCommit: fixture.candidateCommit,
        attestationCommit: fixture.candidateCommit,
        repositoryRoot: fixture.root,
      }),
    /source-functional-binding-stale/u,
  );
});

test("rejects duplicate inventory ids", () => {
  const fixture = attestedFixture();
  fixture.inventory[1] = {
    ...fixture.inventory[1],
    id: fixture.inventory[0].id,
  };
  writeAttestation(fixture, fixture.inventory);
  fixture.attestationCommit = commit(fixture.root, "duplicate inventory");
  assert.throws(
    () =>
      evaluateSourceFunctionalEvidence({
        sourceCommit: fixture.candidateCommit,
        attestationCommit: fixture.attestationCommit,
        repositoryRoot: fixture.root,
      }),
    /source-functional-binding-inventory/u,
  );
});

test("rejects malformed attestation manifests", () => {
  const fixture = attestedFixture();
  writeFileSync(path.join(fixture.root, manifestRelative), "{}\n");
  fixture.attestationCommit = commit(fixture.root, "malformed attestation");
  assert.throws(
    () =>
      evaluateSourceFunctionalEvidence({
        sourceCommit: fixture.candidateCommit,
        attestationCommit: fixture.attestationCommit,
        repositoryRoot: fixture.root,
      }),
    /source-functional-binding-manifest/u,
  );
});

test("rejects substituted retained evidence bytes", () => {
  const fixture = attestedFixture();
  writeFileSync(
    path.join(fixture.evidenceRoot, fixture.inventory[0].path),
    "substituted",
  );
  assert.throws(
    () =>
      evaluateSourceFunctionalEvidence({
        sourceCommit: fixture.candidateCommit,
        attestationCommit: fixture.attestationCommit,
        repositoryRoot: fixture.root,
      }),
    /source-functional-binding-substituted/u,
  );
});

test("rejects evidence whose B blob diverges from the authenticated bytes", () => {
  const fixture = attestedFixture();
  const evidenceFile = path.join(
    fixture.evidenceRoot,
    fixture.inventory[0].path,
  );
  writeFileSync(evidenceFile, "attestation blob swap");
  fixture.attestationCommit = commit(fixture.root, "swapped attestation blob");
  writeFileSync(evidenceFile, fixture.inventory[0].id);
  assert.throws(
    () =>
      evaluateSourceFunctionalEvidence({
        sourceCommit: fixture.candidateCommit,
        attestationCommit: fixture.attestationCommit,
        repositoryRoot: fixture.root,
      }),
    /source-functional-binding-substituted/u,
  );
});

test("rejects inventory evidence absent from B's tree", () => {
  const fixture = attestedFixture();
  rmSync(path.join(fixture.evidenceRoot, fixture.inventory[0].path));
  fixture.attestationCommit = commit(fixture.root, "missing attested blob");
  assert.throws(
    () =>
      evaluateSourceFunctionalEvidence({
        sourceCommit: fixture.candidateCommit,
        attestationCommit: fixture.attestationCommit,
        repositoryRoot: fixture.root,
      }),
    /source-functional-binding-inventory/u,
  );
});

test("rejects evidence symlink escapes", () => {
  const fixture = attestedFixture();
  const outside = path.join(fixture.root, "outside.json");
  writeFileSync(outside, "outside");
  const evidenceFile = path.join(
    fixture.evidenceRoot,
    fixture.inventory[0].path,
  );
  rmSync(evidenceFile);
  symlinkSync(outside, evidenceFile, "file");
  assert.throws(
    () =>
      evaluateSourceFunctionalEvidence({
        sourceCommit: fixture.candidateCommit,
        attestationCommit: fixture.attestationCommit,
        repositoryRoot: fixture.root,
      }),
    /source-functional-binding-inventory/u,
  );
});

test("reports incomplete when the authenticated inventory is missing", () => {
  const fixture = attestedFixture();
  rmSync(path.join(fixture.root, manifestRelative));
  writeFileSync(path.join(fixture.root, "later.txt"), "no manifest\n");
  fixture.attestationCommit = commit(
    fixture.root,
    "attestation without manifest",
  );
  assert.deepEqual(
    evaluateSourceFunctionalEvidence({
      sourceCommit: fixture.candidateCommit,
      attestationCommit: fixture.attestationCommit,
      repositoryRoot: fixture.root,
    }),
    {
      verdict: "incomplete",
      acceptance: "local-green-only",
      reason: "evidence-unbound",
    },
  );
});

test("reports incomplete when the attestation manifest omits inventory", () => {
  const fixture = attestedFixture();
  writeFileSync(
    path.join(fixture.root, manifestRelative),
    `${JSON.stringify({
      candidateCommit: fixture.candidateCommit,
      candidateTree: fixture.candidateTree,
    })}\n`,
  );
  fixture.attestationCommit = commit(
    fixture.root,
    "attestation without inventory",
  );
  assert.deepEqual(
    evaluateSourceFunctionalEvidence({
      sourceCommit: fixture.candidateCommit,
      attestationCommit: fixture.attestationCommit,
      repositoryRoot: fixture.root,
    }),
    {
      verdict: "incomplete",
      acceptance: "local-green-only",
      reason: "evidence-unbound",
    },
  );
});

test("authenticated failed evidence outranks missing evidence", () => {
  const fixture = attestedFixture({ statuses: ["missing", "failed"] });
  assert.equal(
    evaluateSourceFunctionalEvidence({
      sourceCommit: fixture.candidateCommit,
      attestationCommit: fixture.attestationCommit,
      repositoryRoot: fixture.root,
    }).verdict,
    "failed",
  );
});

test("reports the dirty Sage checkout as source-identity-dirty with --attestation", () => {
  const repositoryRoot = path.resolve(
    path.dirname(fileURLToPath(import.meta.url)),
    "..",
  );
  const attestationCommit = git(repositoryRoot, ["rev-parse", "HEAD"]);
  const attestedCandidate = JSON.parse(
    git(repositoryRoot, [
      "show",
      `${attestationCommit}:local-evidence/BETA-0001/source-functional/attestation-v1.json`,
    ]),
  ).candidateCommit;
  assert.deepEqual(
    JSON.parse(
      execFileSync(
        process.execPath,
        [
          path.join(
            repositoryRoot,
            "scripts",
            "beta0001-source-functional-profile.mjs",
          ),
          "--current",
          "--attestation",
          attestationCommit,
        ],
        { cwd: repositoryRoot, encoding: "utf8" },
      ),
    ),
    {
      verdict: "incomplete",
      acceptance: "local-green-only",
      reason: "source-identity-dirty",
      sourceCommit: attestedCandidate,
    },
  );
});
