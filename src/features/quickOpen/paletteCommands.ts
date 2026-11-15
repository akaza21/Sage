import {
  canGoBack,
  canGoForward,
  goBack,
  goForward,
} from "../../app/editorHistory";
import {
  closeActiveEditor,
  isEditorTabKind,
} from "../../app/closeActiveEditor";
import type * as ConflictActions from "../../app/editorConflictActions";
import { useWorkbenchStore } from "../../app/store";
import type { SidebarView } from "../../app/store";
import { formatKeymapChords, keymapBinding } from "../../domain/keymapRegistry";
import { errorMessage } from "../../domain/errors";
import { isDeletedOnDisk } from "../files/editorDisk";
import {
  fileSurfaceCommandAvailable,
  runFileSurfaceCommand,
  type FileSurfaceCommand,
} from "../files/fileSurfaceCommandBus";

const runConflictAction = (
  action: (actions: typeof ConflictActions) => Promise<unknown>,
) =>
  void import("../../app/editorConflictActions")
    .then(action)
    .catch((error: unknown) =>
      useWorkbenchStore.setState({ error: errorMessage(error) }),
    );

export interface PaletteCommand {
  id: string;
  label: string;
  shortcut: string | null;
  /** "files" switches Quick Open back to Files instead of running. */
  run: (() => void) | "files";
}

function shortcutOf(id: string, apple: boolean) {
  return formatKeymapChords(keymapBinding(id), apple)[0] ?? null;
}

const FILE_COMMANDS: Array<[FileSurfaceCommand, string]> = [
  ["goToLine", "Go to Line…"],
  ["goToSymbol", "Go to Symbol in File…"],
  ["find", "Find"],
  ["replace", "Replace"],
  ["save", "Save File"],
];

const VIEWS: Array<[SidebarView, string]> = [
  ["explorer", "Show Explorer"],
  ["search", "Show Search"],
  ["sourceControl", "Show Source Control"],
  ["outline", "Show Outline"],
];

export function paletteCommands(apple: boolean): PaletteCommand[] {
  const store = useWorkbenchStore.getState();
  const activeTab = store.tabs.find((tab) => tab.id === store.activeTabId);
  const commands: PaletteCommand[] = [
    {
      id: "quickOpen",
      label: "Go to File…",
      shortcut: shortcutOf("quickOpen", apple),
      run: "files",
    },
  ];
  for (const [command, label] of FILE_COMMANDS) {
    if (!fileSurfaceCommandAvailable(command)) continue;
    commands.push({
      id: command,
      label,
      shortcut: shortcutOf(command, apple),
      run: () => void runFileSurfaceCommand(command),
    });
  }
  const editor = store.activeEditor;
  if (
    fileSurfaceCommandAvailable("save") &&
    activeTab?.kind === "file" &&
    activeTab.dirty &&
    editor?.phase === "ready" &&
    !isDeletedOnDisk(editor)
  ) {
    commands.push({
      id: "revert",
      label: "Revert File",
      shortcut: null,
      // The File shown now, even if another is active once the action runs.
      run: () =>
        runConflictAction((a) => a.reloadActiveEditor(editor.resource)),
    });
  }
  if (store.tabs.some((tab) => tab.kind === "file" && tab.dirty)) {
    commands.push({
      id: "saveAll",
      label: "Save All",
      shortcut: shortcutOf("saveAll", apple),
      run: () => runConflictAction((a) => a.saveAllEditors()),
    });
  }
  if (canGoBack()) {
    commands.push({
      id: "navigateBack",
      label: "Go Back",
      shortcut: shortcutOf("navigateBack", apple),
      run: () => void goBack(),
    });
  }
  if (canGoForward()) {
    commands.push({
      id: "navigateForward",
      label: "Go Forward",
      shortcut: shortcutOf("navigateForward", apple),
      run: () => void goForward(),
    });
  }
  for (const [view, label] of VIEWS) {
    commands.push({
      id: `view.${view}`,
      label,
      shortcut: null,
      run: () => useWorkbenchStore.getState().setSidebarView(view),
    });
  }
  if (activeTab?.kind !== "settings") {
    commands.push({
      id: "openSettings",
      label: "Open Settings",
      shortcut: shortcutOf("openSettings", apple),
      run: () => void useWorkbenchStore.getState().openSettings(),
    });
  }
  if (activeTab && isEditorTabKind(activeTab.kind)) {
    commands.push({
      id: "closeEditor",
      label: "Close Editor",
      shortcut: shortcutOf("closeEditor", apple),
      // The tab active when it runs, which the list may not have seen.
      run: closeActiveEditor,
    });
  }
  return commands;
}

export function filterCommands(
  commands: readonly PaletteCommand[],
  query: string,
): PaletteCommand[] {
  const words = query.toLowerCase().split(/\s+/u).filter(Boolean);
  return commands.filter((command) => {
    const label = command.label.toLowerCase();
    return words.every((word) => label.includes(word));
  });
}
