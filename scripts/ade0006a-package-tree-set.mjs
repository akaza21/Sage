import path from "node:path";

import { recordDigest, stableJson } from "./ade0006a-c0b-common.mjs";
import { observeAuthenticatedPackageTree } from "./ade0006a-package-tree.mjs";

function validateMembers(members, bounds) {
  if (
    !Array.isArray(members) ||
    members.length < 1 ||
    !Number.isSafeInteger(bounds?.members) ||
    !Number.isSafeInteger(bounds?.bytes)
  ) {
    throw new Error("authenticated package tree set is invalid");
  }
  const labels = new Set();
  const roots = new Set();
  for (const member of members) {
    const resolved = path.resolve(member?.root ?? "");
    if (
      !/^[a-z][a-z0-9-]{0,63}$/u.test(member?.label ?? "") ||
      !path.isAbsolute(member?.root ?? "") ||
      labels.has(member.label) ||
      roots.has(resolved)
    ) {
      throw new Error("authenticated package tree set member is invalid");
    }
    labels.add(member.label);
    roots.add(resolved);
  }
}

export function resolveAuthenticatedPackageTreeSetMembers({
  root,
  members,
  variables = {},
}) {
  return members.map(({ label, relativePath }) => {
    let resolvedPath = relativePath;
    for (const [name, value] of Object.entries(variables)) {
      resolvedPath = resolvedPath.replaceAll(`{${name}}`, value);
    }
    if (resolvedPath.includes("{") || resolvedPath.includes("}")) {
      throw new Error("authenticated package tree set variable is unresolved");
    }
    return { label, root: path.join(root, ...resolvedPath.split("/")) };
  });
}

function summarize(entries, bounds) {
  const fileCount = entries.reduce(
    (total, entry) => total + entry.tree.summary.fileCount,
    0,
  );
  const bytes = entries.reduce(
    (total, entry) => total + entry.tree.summary.bytes,
    0,
  );
  if (
    !Number.isSafeInteger(fileCount) ||
    fileCount > bounds.members ||
    !Number.isSafeInteger(bytes) ||
    bytes > bounds.bytes
  ) {
    throw new Error("authenticated package tree set exceeds its bounds");
  }
  return {
    treeSha256: recordDigest(
      entries.map(({ label, tree }) => ({ label, ...tree.summary })),
    ),
    fileCount,
    bytes,
  };
}

export async function observeAuthenticatedPackageTreeSet({ members, bounds }) {
  validateMembers(members, bounds);
  const entries = await Promise.all(
    members.map(async ({ label, root }) => ({
      label,
      tree: await observeAuthenticatedPackageTree({ root, bounds }),
    })),
  );
  entries.sort((left, right) => left.label.localeCompare(right.label, "en"));
  return { entries, summary: summarize(entries, bounds) };
}

export async function verifyAuthenticatedPackageTreeSet(treeSet, bounds) {
  validateMembers(
    treeSet?.entries?.map(({ label, tree }) => ({ label, root: tree.root })),
    bounds,
  );
  const observed = await observeAuthenticatedPackageTreeSet({
    members: treeSet.entries.map(({ label, tree }) => ({
      label,
      root: tree.root,
    })),
    bounds,
  });
  if (
    stableJson(observed.summary) !== stableJson(treeSet.summary) ||
    observed.entries.some(
      (entry, index) =>
        stableJson(entry.tree.records) !==
        stableJson(treeSet.entries[index].tree.records),
    )
  ) {
    throw new Error("authenticated package tree set changed");
  }
}
