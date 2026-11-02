import { create } from "zustand";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { createSettingWriter } from "../app/settingWriteQueue";
import {
  EDITOR_TOOL_RAIL_WIDTH,
  clampWorkbenchSidebarWidth,
} from "./workbenchSidebarResize";
import {
  DEFAULT_WORKBENCH_WINDOW_MODE,
  isWorkbenchWindowMode,
  type WorkbenchWindowMode,
} from "./workbenchWindowMode";

interface TerminalDockStore {
  visible: boolean;
  ready: boolean;
  error: string | null;
  restore: () => Promise<void>;
  flush: () => Promise<void>;
  setVisible: (visible: boolean) => void;
}

const writes = createSettingWriter(false);

/** Disposable projection of the native dock preference; sessions own their lifecycle. */
export const useTerminalDockStore = create<TerminalDockStore>((set, get) => ({
  visible: false,
  ready: false,
  error: null,
  restore: async () => {
    await writes.idle();
    try {
      const visible =
        (await getWorkbenchPorts().settings.getTerminalDockVisible()) === true;
      writes.persisted(visible);
      set({ visible, ready: true, error: null });
    } catch {
      set({ ready: false, error: "Terminal visibility could not be loaded." });
    }
  },
  flush: () => writes.idle(),
  setVisible: (visible) => {
    if (!get().ready) return;
    set({ visible, error: null });
    writes.write(
      visible,
      (value) => getWorkbenchPorts().settings.setTerminalDockVisible(value),
      (persisted) =>
        set({
          visible: persisted,
          error: "Terminal visibility could not be saved.",
        }),
    );
  },
}));

export const WORKBENCH_LAYOUT_STORAGE_KEY = "sage.workbench.layout.v1";

export type WorkbenchLayout = {
  editorToolRailWidth: number;
  windowMode: WorkbenchWindowMode;
};

export function defaultWorkbenchLayout(): WorkbenchLayout {
  return {
    editorToolRailWidth: EDITOR_TOOL_RAIL_WIDTH.defaultWidth,
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
    (typeof layout.windowMode === "undefined" ||
      typeof layout.windowMode === "string")
  );
}
