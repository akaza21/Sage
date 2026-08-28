import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { lstatSync, readFileSync, realpathSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptsDirectory = path.dirname(fileURLToPath(import.meta.url));
const profilePath = path.join(
  path.dirname(scriptsDirectory),
  "docs",
  "work-items",
  "BETA-0001-primary-beta-path",
  "contracts",
  "beta-source-functional-evidence-v1.json",
);

function invalid(code) {
  throw new Error(`source-functional-${code}`);
}

function validCommit(value) {
  return typeof value === "string" && /^[0-9a-f]{40}$/u.test(value);
}
function validDigest(value) {
  return typeof value === "string" && /^[0-9a-f]{64}$/u.test(value);
}

function loadProfile() {
  const profile = JSON.parse(readFileSync(profilePath, "utf8"));
  const requiredKeys = [
    "acceptance",
    "deferredEvidence",
    "evidenceBindingRequired",
    "requiredEvidence",
    "schemaVersion",
    "status",
    "workItem",
  ];
  if (
    !profile ||
    typeof profile !== "object" ||
    Array.isArray(profile) ||
    JSON.stringify(Object.keys(profile).sort()) !==
      JSON.stringify(requiredKeys.sort()) ||
    profile.schemaVersion !== 1 ||
    profile.status !== "frozen-source-functional-verdict-profile" ||
    profile.workItem !== "BETA-0001" ||
    profile.acceptance !== "local-green-only" ||
    profile.evidenceBindingRequired !== true ||
    !Array.isArray(profile.deferredEvidence) ||
    !Array.isArray(profile.requiredEvidence)
  ) {
    invalid("profile");
  }
  const all = [...profile.deferredEvidence, ...profile.requiredEvidence];
  if (
    all.some(
      (id) =>
        typeof id !== "string" || !/^[a-z0-9]+(?:[.-][a-z0-9]+)*$/u.test(id),
    ) ||
    new Set(all).size !== all.length ||
    profile.deferredEvidence.join("\0") !==
      "windows.source-package\0windows.installed-lifecycle" ||
    profile.requiredEvidence.length === 0
  ) {
    invalid("profile");
  }
  return Object.freeze({
    ...profile,
    deferredEvidence: Object.freeze(profile.deferredEvidence),
    requiredEvidence: Object.freeze(profile.requiredEvidence),
  });
}

export const SOURCE_FUNCTIONAL_PROFILE = loadProfile();
export const SOURCE_FUNCTIONAL_REQUIRED_IDS =
  SOURCE_FUNCTIONAL_PROFILE.requiredEvidence;

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

const retainedEvidenceRelative = "local-evidence/BETA-0001/source-functional";
const manifestRelative = `${retainedEvidenceRelative}/attestation-v1.json`;

function git(repositoryRoot, args, encoding = "utf8") {
  return execFileSync("git", args, { cwd: repositoryRoot, encoding }).trim();
}

function requireObjectType(repositoryRoot, objectId, type) {
  try {
    return git(repositoryRoot, ["cat-file", "-t", objectId]) === type;
  } catch {
    return false;
  }
}

function committedManifest(repositoryRoot, attestationCommit) {
  try {
    return execFileSync(
      "git",
      ["show", `${attestationCommit}:${manifestRelative}`],
      { cwd: repositoryRoot, stdio: ["ignore", "pipe", "ignore"] },
    );
  } catch {
    return null;
  }
}

function committedEvidenceBytes(
  repositoryRoot,
  attestationCommit,
  relativePath,
) {
  try {
    return execFileSync(
      "git",
      [
        "show",
        `${attestationCommit}:${retainedEvidenceRelative}/${relativePath}`,
      ],
      { cwd: repositoryRoot, stdio: ["ignore", "pipe", "ignore"] },
    );
  } catch {
    return null;
  }
}

function candidateCommitFromAttestation(repositoryRoot, attestationCommit) {
  if (!validCommit(attestationCommit)) return undefined;
  const manifestBytes = committedManifest(repositoryRoot, attestationCommit);
  if (!manifestBytes) return undefined;
  try {
    const manifest = JSON.parse(manifestBytes.toString("utf8"));
    return validCommit(manifest?.candidateCommit)
      ? manifest.candidateCommit
      : undefined;
  } catch {
    return undefined;
  }
}

function validInventory(inventory) {
  if (
    !Array.isArray(inventory) ||
    inventory.length !== SOURCE_FUNCTIONAL_REQUIRED_IDS.length
  )
    invalid("binding-inventory");
  const seen = new Set();
  for (const [index, item] of inventory.entries()) {
    if (
      !item ||
      typeof item !== "object" ||
      Array.isArray(item) ||
      Object.keys(item).sort().join("\0") !== "id\0path\0sha256\0status" ||
      item.id !== SOURCE_FUNCTIONAL_REQUIRED_IDS[index] ||
      !["passed", "failed", "missing"].includes(item.status) ||
      typeof item.path !== "string" ||
      !/^[a-z0-9][a-z0-9._/-]*$/u.test(item.path) ||
      item.path.includes("..") ||
      !validDigest(item.sha256) ||
      seen.has(item.id)
    )
      invalid("binding-inventory");
    seen.add(item.id);
  }
}

export function authenticateSourceFunctionalEvidence({
  sourceCommit,
  attestationCommit,
  repositoryRoot = path.dirname(scriptsDirectory),
}) {
  if (!validCommit(sourceCommit)) invalid("candidate");
  if (attestationCommit === undefined) return null;
  if (!validCommit(attestationCommit)) invalid("attestation");
  if (
    !requireObjectType(repositoryRoot, sourceCommit, "commit") ||
    !requireObjectType(repositoryRoot, attestationCommit, "commit")
  )
    invalid("binding-stale");

  let sourceTree;
  let attestationTree;
  try {
    sourceTree = git(repositoryRoot, ["rev-parse", `${sourceCommit}^{tree}`]);
    attestationTree = git(repositoryRoot, [
      "rev-parse",
      `${attestationCommit}^{tree}`,
    ]);
    if (
      !requireObjectType(repositoryRoot, sourceTree, "tree") ||
      !requireObjectType(repositoryRoot, attestationTree, "tree") ||
      sourceCommit === attestationCommit
    )
      invalid("binding-stale");
    git(repositoryRoot, [
      "merge-base",
      "--is-ancestor",
      sourceCommit,
      attestationCommit,
    ]);
  } catch {
    invalid("binding-stale");
  }

  const manifestBytes = committedManifest(repositoryRoot, attestationCommit);
  if (!manifestBytes) return null;
  let manifest;
  try {
    manifest = JSON.parse(manifestBytes.toString("utf8"));
  } catch {
    invalid("binding-manifest");
  }
  if (
    manifest &&
    typeof manifest === "object" &&
    !Array.isArray(manifest) &&
    Object.keys(manifest).sort().join("\0") === "candidateCommit\0candidateTree"
  )
    return null;
  if (
    !manifest ||
    typeof manifest !== "object" ||
    Array.isArray(manifest) ||
    Object.keys(manifest).sort().join("\0") !==
      "candidateCommit\0candidateTree\0inventory" ||
    manifest.candidateCommit !== sourceCommit ||
    manifest.candidateTree !== sourceTree
  )
    invalid("binding-manifest");
  validInventory(manifest.inventory);

  const evidenceRoot = path.resolve(repositoryRoot, retainedEvidenceRelative);
  let canonicalEvidenceRoot;
  try {
    if (lstatSync(evidenceRoot).isSymbolicLink()) invalid("binding-inventory");
    canonicalEvidenceRoot = realpathSync(evidenceRoot);
  } catch {
    return null;
  }
  for (const item of manifest.inventory) {
    const file = path.resolve(evidenceRoot, item.path);
    if (!file.startsWith(`${evidenceRoot}${path.sep}`))
      invalid("binding-inventory");
    const blobBytes = committedEvidenceBytes(
      repositoryRoot,
      attestationCommit,
      item.path,
    );
    if (!blobBytes) invalid("binding-inventory");
    if (sha256(blobBytes) !== item.sha256) invalid("binding-substituted");
    let bytes;
    try {
      const stat = lstatSync(file);
      if (
        stat.isSymbolicLink() ||
        !stat.isFile() ||
        !realpathSync(file).startsWith(`${canonicalEvidenceRoot}${path.sep}`)
      )
        invalid("binding-inventory");
      bytes = readFileSync(file);
    } catch (error) {
      if (error?.message?.startsWith("source-functional-")) throw error;
      return null;
    }
    if (!bytes.equals(blobBytes)) invalid("binding-substituted");
  }
  return Object.freeze({
    sourceCommit,
    sourceTree,
    attestationCommit,
    attestationTree,
    evidence: manifest.inventory.map(({ id, status }) => ({ id, status })),
  });
}

export function evaluateSourceFunctionalEvidence({
  sourceCommit,
  attestationCommit,
  repositoryRoot,
}) {
  const binding = authenticateSourceFunctionalEvidence({
    sourceCommit,
    attestationCommit,
    ...(repositoryRoot ? { repositoryRoot } : {}),
  });
  if (!binding) {
    return {
      verdict: "incomplete",
      acceptance: SOURCE_FUNCTIONAL_PROFILE.acceptance,
      reason: "evidence-unbound",
    };
  }
  if (binding.evidence.some((descriptor) => descriptor.status === "failed")) {
    return {
      verdict: "failed",
      acceptance: SOURCE_FUNCTIONAL_PROFILE.acceptance,
    };
  }
  if (binding.evidence.some((descriptor) => descriptor.status === "missing")) {
    return {
      verdict: "incomplete",
      acceptance: SOURCE_FUNCTIONAL_PROFILE.acceptance,
      reason: "evidence-missing",
    };
  }
  return {
    verdict: "passed",
    acceptance: SOURCE_FUNCTIONAL_PROFILE.acceptance,
  };
}

export function evaluateSourceFunctionalWorkingTree({
  sourceCommit,
  attestationCommit,
  workingTreeClean,
  repositoryRoot,
}) {
  if (!validCommit(sourceCommit) || typeof workingTreeClean !== "boolean") {
    invalid("candidate");
  }
  if (!workingTreeClean) {
    return {
      verdict: "incomplete",
      acceptance: SOURCE_FUNCTIONAL_PROFILE.acceptance,
      reason: "source-identity-dirty",
    };
  }
  return evaluateSourceFunctionalEvidence({
    sourceCommit,
    attestationCommit,
    ...(repositoryRoot ? { repositoryRoot } : {}),
  });
}

export function currentWorkingTreeResult({
  repositoryRoot = path.dirname(scriptsDirectory),
  attestationCommit,
} = {}) {
  const headCommit = execFileSync("git", ["rev-parse", "HEAD"], {
    cwd: repositoryRoot,
    encoding: "utf8",
  }).trim();
  const sourceCommit =
    candidateCommitFromAttestation(repositoryRoot, attestationCommit) ??
    headCommit;
  const workingTreeClean =
    execFileSync("git", ["status", "--porcelain"], {
      cwd: repositoryRoot,
      encoding: "utf8",
    }).trim() === "";
  return {
    sourceCommit,
    ...evaluateSourceFunctionalWorkingTree({
      sourceCommit,
      attestationCommit,
      workingTreeClean,
      repositoryRoot,
    }),
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const arguments_ = process.argv.slice(2);
  const isCurrent = arguments_.length === 1 && arguments_[0] === "--current";
  const hasAttestation =
    arguments_.length === 3 &&
    arguments_[0] === "--current" &&
    arguments_[1] === "--attestation";
  if (!isCurrent && !hasAttestation) {
    throw new Error(
      "usage: beta0001-source-functional-profile.mjs --current [--attestation <commit>]",
    );
  }
  process.stdout.write(
    `${JSON.stringify(
      currentWorkingTreeResult({
        ...(hasAttestation ? { attestationCommit: arguments_[2] } : {}),
      }),
    )}\n`,
  );
}
