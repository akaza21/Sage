export const WORKBENCH_WINDOW_MODES = ["agent", "editor"] as const;

export type WorkbenchWindowMode = (typeof WORKBENCH_WINDOW_MODES)[number];

export const DEFAULT_WORKBENCH_WINDOW_MODE: WorkbenchWindowMode = "editor";

const workbenchWindowModes = new Set<string>(WORKBENCH_WINDOW_MODES);

export function isWorkbenchWindowMode(
  value: unknown,
): value is WorkbenchWindowMode {
  return typeof value === "string" && workbenchWindowModes.has(value);
}
