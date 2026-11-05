export const COMPACT_WORKBENCH_QUERY = "(max-width: 760px)";

/** The compact tool rail covers the Editor canvas until navigation dismisses it. */
export function isCompactWorkbenchViewport() {
  return Boolean(window.matchMedia?.(COMPACT_WORKBENCH_QUERY).matches);
}
