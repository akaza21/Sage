import {
  createElement,
  useEffect,
  useEffectEvent,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";

import { DISCONNECTED_TERMINAL } from "./terminalDockProjection";
import { isTauriRuntime } from "../ports/nativeInvoke";
import { useWorkbenchStore } from "../app/store";
import { keymapBinding, matchesPlatformChord } from "../domain/keymapRegistry";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import {
  closeQuickOpen,
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
  // Own the Editor lifetime even while its deferred UI is still loading.
  useEffect(() => {
    if (windowMode === "editor") return closeQuickOpen;
  }, [windowMode]);
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
      // In Agent mode the same chord opens the Chat's Files picker.
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

/** The terminal controller loads when a native workbench is available. */
export function useTerminalDock(blocked: boolean) {
  const [terminal, setTerminal] = useState(DISCONNECTED_TERMINAL);
  const isBlocked = useEffectEvent(() => blocked);
  const native = isTauriRuntime();
  useEffect(() => {
    if (!native) return;
    let cancelled = false;
    let dispose: (() => void) | undefined;
    void import("./terminalDockCommands").then(
      ({ connectTerminalDock }) => {
        if (!cancelled)
          dispose = connectTerminalDock(() => isBlocked(), setTerminal);
      },
      () => {
        if (!cancelled)
          setTerminal({
            ...DISCONNECTED_TERMINAL,
            controls: createElement(
              "span",
              { role: "status" },
              "Terminal failed to load.",
            ),
          });
      },
    );
    return () => {
      cancelled = true;
      dispose?.();
    };
  }, [native]);
  return terminal;
}
