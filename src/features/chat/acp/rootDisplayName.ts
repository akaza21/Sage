export function rootDisplayName(
  rootLabels: Readonly<Record<string, string>>,
  rootId: string,
): string {
  const projected = Object.hasOwn(rootLabels, rootId)
    ? rootLabels[rootId]
    : null;
  return typeof projected === "string" && projected.length > 0
    ? projected
    : rootId;
}
