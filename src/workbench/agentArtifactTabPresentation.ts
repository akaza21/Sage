type ArtifactTab = { resource?: { relativePath?: string }; title: string };

export function artifactTabParts(tab: ArtifactTab) {
  const full = tab.resource?.relativePath ?? tab.title;
  const base = full.split("/").at(-1) ?? full;
  return [full, base, full.slice(0, full.length - base.length)];
}

export function activeArtifactForTarget<T extends { id: string }>(
  artifacts: readonly T[],
  target: { kind: string; tabId?: string } | null,
): T | null {
  return target?.kind === "tab"
    ? (artifacts.find((artifact) => artifact.id === target.tabId) ?? null)
    : null;
}
