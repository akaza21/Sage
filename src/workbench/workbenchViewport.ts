export const COMPACT_WORKBENCH_QUERY = "(max-width: 760px)";

export function isCompactWorkbenchViewport() {
  return Boolean(window.matchMedia?.(COMPACT_WORKBENCH_QUERY).matches);
}
