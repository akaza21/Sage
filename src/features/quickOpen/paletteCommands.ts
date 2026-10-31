// The command palette's commands (ED-2a): a short list of what the Editor
// can already do, each run exactly as its own key or button runs it.

import { useWorkbenchStore } from "../../app/store";
import type { SidebarView } from "../../app/store";
import { formatKeymapChords, keymapBinding } from "../../domain/keymapRegistry";
import {
  hasFileSurface,
  runFileSurfaceCommand,
  type FileSurfaceCommand,
} from "../files/fileSurfaceCommandBus";

export interface PaletteCommand {
  id: string;
  label: string;
  /** The command's own shortcut, as the Keyboard settings page shows it. */
  shortcut: string | null;
  /** "files" switches Quick Open back to Files instead of running. */
  run: (() => void) | "files";
  /** Whether running it moves the focus itself, so the box shouldn't. */
  takesFocus: boolean;
}

function shortcutOf(id: string, apple: boolean) {
  return formatKeymapChords(keymapBinding(id), apple)[0] ?? null;
}

const FILE_COMMANDS: Array<[FileSurfaceCommand, string, string, boolean]> = [
  ["goToLine", "Go to Line…", "goToLine", true],
  ["goToSymbol", "Go to Symbol in File…", "goToSymbol", true],
  ["find", "Find", "find", true],
  ["replace", "Replace", "replace", true],
  ["save", "Save File", "save", false],
];

const VIEWS: Array<[SidebarView, string]> = [
  ["explorer", "Show Explorer"],
  ["search", "Show Search"],
  ["sourceControl", "Show Source Control"],
  ["outline", "Show Outline"],
];

/** The commands that can run now, in a fixed order. */
export function paletteCommands(apple: boolean): PaletteCommand[] {
  const store = useWorkbenchStore.getState();
  const activeTab = store.tabs.find((tab) => tab.id === store.activeTabId);
  const commands: PaletteCommand[] = [
    {
      id: "quickOpen",
      label: "Go to File…",
      shortcut: shortcutOf("quickOpen", apple),
      run: "files",
      takesFocus: true,
    },
  ];
  if (hasFileSurface()) {
    for (const [command, label, binding, takesFocus] of FILE_COMMANDS) {
      commands.push({
        id: command,
        label,
        shortcut: shortcutOf(binding, apple),
        run: () => void runFileSurfaceCommand(command),
        takesFocus,
      });
    }
  }
  for (const [view, label] of VIEWS) {
    commands.push({
      id: `view.${view}`,
      label,
      shortcut: null,
      run: () => useWorkbenchStore.getState().setSidebarView(view),
      takesFocus: false,
    });
  }
  commands.push({
    id: "openSettings",
    label: "Open Settings",
    shortcut: shortcutOf("openSettings", apple),
    run: () => void useWorkbenchStore.getState().openSettings(),
    takesFocus: true,
  });
  if (
    activeTab &&
    (activeTab.kind === "file" ||
      activeTab.kind === "diff" ||
      activeTab.kind === "settings")
  ) {
    commands.push({
      id: "closeEditor",
      label: "Close Editor",
      shortcut: null,
      run: () => useWorkbenchStore.getState().closeTab(activeTab.id),
      takesFocus: false,
    });
  }
  return commands;
}

/** Commands whose label holds every word typed, in any order. */
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
