import {
  EDITOR_TOOL_RAIL_WIDTH,
  clampWorkbenchSidebarWidth,
} from "./workbenchSidebarResize";
import {
  AGENT_CHAT_SPLIT_DEFAULT,
  AGENT_CHAT_SPLIT_MAX,
  AGENT_CHAT_SPLIT_MIN,
} from "./agentChatSplit";
import {
  DEFAULT_WORKBENCH_WINDOW_MODE,
  isWorkbenchWindowMode,
  type WorkbenchWindowMode,
} from "./workbenchWindowMode";

export const WORKBENCH_LAYOUT_STORAGE_KEY = "sage.workbench.layout.v1";

export type WorkbenchLayout = {
  editorToolRailWidth: number;
  agentChatSplitPercent: number;
  windowMode: WorkbenchWindowMode;
};

export function defaultWorkbenchLayout(): WorkbenchLayout {
  return {
    editorToolRailWidth: EDITOR_TOOL_RAIL_WIDTH.defaultWidth,
    agentChatSplitPercent: AGENT_CHAT_SPLIT_DEFAULT,
    windowMode: DEFAULT_WORKBENCH_WINDOW_MODE,
  };
}

export function readWorkbenchLayout(): WorkbenchLayout {
  const fallback = defaultWorkbenchLayout();
  try {
    const stored = localStorage.getItem(WORKBENCH_LAYOUT_STORAGE_KEY);
    if (stored === null) return fallback;
    const parsed: unknown = JSON.parse(stored);
    if (!isLayoutRecord(parsed)) return fallback;
    return normalizeWorkbenchLayout(parsed);
  } catch {
    return fallback;
  }
}

export function writeWorkbenchLayout(layout: Partial<WorkbenchLayout>) {
  try {
    localStorage.setItem(
      WORKBENCH_LAYOUT_STORAGE_KEY,
      JSON.stringify(
        normalizeWorkbenchLayout({ ...readWorkbenchLayout(), ...layout }),
      ),
    );
  } catch {
    // Private or quota-limited WebViews retain a renderer-local layout.
  }
}

function normalizeWorkbenchLayout(layout: WorkbenchLayout): WorkbenchLayout {
  return {
    editorToolRailWidth: clampWorkbenchSidebarWidth(
      EDITOR_TOOL_RAIL_WIDTH,
      layout.editorToolRailWidth,
    ),
    agentChatSplitPercent: clampSplitPercent(layout.agentChatSplitPercent),
    windowMode: isWorkbenchWindowMode(layout.windowMode)
      ? layout.windowMode
      : DEFAULT_WORKBENCH_WINDOW_MODE,
  };
}

function isLayoutRecord(value: unknown): value is WorkbenchLayout {
  if (typeof value !== "object" || value === null) return false;
  const layout = value as Record<string, unknown>;
  return (
    typeof layout.editorToolRailWidth === "number" &&
    typeof layout.agentChatSplitPercent === "number" &&
    (typeof layout.windowMode === "undefined" ||
      typeof layout.windowMode === "string")
  );
}

function clampSplitPercent(value: number): number {
  if (!Number.isFinite(value)) return AGENT_CHAT_SPLIT_DEFAULT;
  return Math.min(
    AGENT_CHAT_SPLIT_MAX,
    Math.max(AGENT_CHAT_SPLIT_MIN, Math.round(value)),
  );
}
