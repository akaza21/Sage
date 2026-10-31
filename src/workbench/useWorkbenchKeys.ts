import { useEffect, type Dispatch, type SetStateAction } from "react";

import { useWorkbenchStore } from "../app/store";
import { keymapBinding, matchesPlatformChord } from "../domain/keymapRegistry";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import {
  openCommandPalette,
  openQuickOpen,
} from "../features/quickOpen/quickOpenState";
import { usesMetaModifier } from "../ui/shortcutLabel";
import type { WorkbenchWindowMode } from "./workbenchWindowMode";

/** Window-level commands shared by Agent and Editor, guarded by resource modals. */
export function useWorkbenchKeys({
  narrowAgent,
  resourceModal,
  windowMode,
  setNavigation,
  setWindowMode,
}: {
  narrowAgent: boolean;
  resourceModal: boolean;
  windowMode: WorkbenchWindowMode;
  setNavigation: Dispatch<
    SetStateAction<{ narrow: boolean; collapsed: boolean }>
  >;
  setWindowMode: Dispatch<SetStateAction<WorkbenchWindowMode>>;
}) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (resourceModal) return;
      // Platform modifier only: Control+N on macOS is the editor's next line.
      const apple = usesMetaModifier();
      if (matchesPlatformChord(keymapBinding("toggleSidebar"), event, apple)) {
        event.preventDefault();
        if (windowMode === "agent")
          setNavigation((value) => ({
            narrow: narrowAgent,
            collapsed: !(value.narrow === narrowAgent
              ? value.collapsed
              : narrowAgent),
          }));
        else useWorkbenchStore.getState().toggleSidebar();
      }
      if (matchesPlatformChord(keymapBinding("openSettings"), event, apple)) {
        event.preventDefault();
        if (windowMode === "agent") useAgentSettingsState.getState().open();
        else void useWorkbenchStore.getState().openSettings();
      }
      // In Agent mode the same chord opens the Chat's Files picker (CP-48).
      if (
        windowMode === "editor" &&
        matchesPlatformChord(keymapBinding("quickOpen"), event, apple)
      ) {
        event.preventDefault();
        openQuickOpen();
      }
      if (
        windowMode === "editor" &&
        matchesPlatformChord(keymapBinding("commandPalette"), event, apple)
      ) {
        event.preventDefault();
        openCommandPalette();
      }
      if (matchesPlatformChord(keymapBinding("newTask"), event, apple)) {
        event.preventDefault();
        useAgentSettingsState.getState().close();
        setWindowMode("agent");
        void useWorkbenchStore.getState().createChat();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [narrowAgent, resourceModal, setNavigation, setWindowMode, windowMode]);
}
