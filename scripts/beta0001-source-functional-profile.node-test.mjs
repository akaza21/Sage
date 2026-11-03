import assert from "node:assert/strict";
import test, { afterEach } from "node:test";
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  mkdirSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
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
const cliRelative = "scripts/beta0001-source-functional-profile.mjs";
const fixtureRoots = [];

afterEach(() => {
  for (const root of fixtureRoots.splice(0)) {
    rmSync(root, { recursive: true, force: true });
  }
});

function runCli(
  fixture,
  args = ["--current", "--attestation", fixture.attestationCommit],
) {
  return spawnSync(
    process.execPath,
    [path.join(fixture.root, cliRelative), ...args],
    { cwd: fixture.root, encoding: "utf8" },
  );
}

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

function attestedFixture({ statuses, cli = false } = {}) {
  const root = mkdtempSync(path.join(tmpdir(), "sage-source-evidence-"));
  fixtureRoots.push(root);
  execFileSync("git", ["init"], { cwd: root });
  // The CLI locates its repository and contract relative to its own module.
  // Install the real files before candidate A so both A and B stay clean.
  if (cli) {
    const repositoryRoot = fileURLToPath(new URL("../", import.meta.url));
    for (const relative of [
      cliRelative,
      "docs/work-items/BETA-0001-primary-beta-path/contracts/beta-source-functional-evidence-v1.json",
    ]) {
      const destination = path.join(root, relative);
      mkdirSync(path.dirname(destination), { recursive: true });
      copyFileSync(path.join(repositoryRoot, relative), destination);
    }
  }
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

test("current evaluation rejects a dirty checkout before evidence", () => {
  const fixture = attestedFixture();
  writeFileSync(path.join(fixture.root, "dirty.txt"), "dirty\n");
  assert.deepEqual(
    currentWorkingTreeResult({
      repositoryRoot: fixture.root,
      attestationCommit: fixture.attestationCommit,
    }),
    {
      sourceCommit: fixture.candidateCommit,
      verdict: "incomplete",
      acceptance: "local-green-only",
      reason: "source-identity-dirty",
    },
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

test("CLI reports the current checkout state with --attestation", () => {
  const fixture = attestedFixture({ cli: true });
  const result = runCli(fixture);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), {
    sourceCommit: fixture.candidateCommit,
    verdict: "passed",
    acceptance: "local-green-only",
  });
});

test("CLI reports HEAD as unbound without --attestation", () => {
  const fixture = attestedFixture({ cli: true });
  const result = runCli(fixture, ["--current"]);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), {
    sourceCommit: fixture.attestationCommit,
    verdict: "incomplete",
    acceptance: "local-green-only",
    reason: "evidence-unbound",
  });
});

test("CLI rejects a non-ancestor candidate even with the same source tree", () => {
  const fixture = attestedFixture({ cli: true });
  // Model rewritten history without changing the candidate's contents.
  fixture.candidateCommit = git(fixture.root, [
    "-c",
    "user.name=test",
    "-c",
    "user.email=test@example.invalid",
    "commit-tree",
    fixture.candidateTree,
    "-m",
    "unrelated candidate",
  ]);
  writeAttestation(fixture, fixture.inventory);
  fixture.attestationCommit = commit(fixture.root, "stale attestation");
  const result = runCli(fixture);
  assert.equal(result.status, 1, result.stderr);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /source-functional-binding-stale/u);
});

for (const file of ["pinned.txt", "untracked.txt"]) {
  test(`CLI reports dirty source identity for ${file}`, () => {
    const fixture = attestedFixture({ cli: true });
    writeFileSync(path.join(fixture.root, file), "dirty\n");
    const result = runCli(fixture);
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(JSON.parse(result.stdout), {
      sourceCommit: fixture.candidateCommit,
      verdict: "incomplete",
      acceptance: "local-green-only",
      reason: "source-identity-dirty",
    });
  });
}
