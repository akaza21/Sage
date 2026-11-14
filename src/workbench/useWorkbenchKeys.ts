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
import { closeActiveEditor } from "../app/closeActiveEditor";
import { useWorkbenchStore } from "../app/store";
import { errorMessage } from "../domain/errors";
import { keymapBinding, matchesPlatformChord } from "../domain/keymapRegistry";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import {
  closeQuickOpen,
  openCommandPalette,
  openQuickOpen,
} from "../features/quickOpen/quickOpenState";
import { usesMetaModifier } from "../ui/shortcutLabel";
import type { WorkbenchWindowMode } from "./workbenchWindowMode";

export function useWorkbenchKeys({
  resourceModal,
  windowMode,
  toggleAgentNavigation,
  setWindowMode,
}: {
  resourceModal: boolean;
  windowMode: WorkbenchWindowMode;
  toggleAgentNavigation: () => void;
  setWindowMode: Dispatch<SetStateAction<WorkbenchWindowMode>>;
}) {
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
        if (windowMode === "agent") toggleAgentNavigation();
        else useWorkbenchStore.getState().toggleSidebar();
      }
      if (matchesPlatformChord(keymapBinding("openSettings"), event, apple)) {
        event.preventDefault();
        if (windowMode === "agent") useAgentSettingsState.getState().open();
        else void useWorkbenchStore.getState().openSettings();
      }
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
      if (
        windowMode === "editor" &&
        matchesPlatformChord(keymapBinding("saveAll"), event, apple)
      ) {
        event.preventDefault();
        void import("../app/editorConflictActions")
          .then((actions) => actions.saveAllEditors())
          .catch((error: unknown) =>
            useWorkbenchStore.setState({ error: errorMessage(error) }),
          );
      }
      // A terminal keeps Ctrl+W as its own delete-word: xterm consumes it
      // before it bubbles here. Cmd+W on macOS reaches here, as in VS Code.
      if (
        windowMode === "editor" &&
        !event.defaultPrevented &&
        matchesPlatformChord(keymapBinding("closeEditor"), event, apple)
      ) {
        event.preventDefault();
        closeActiveEditor();
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
  }, [resourceModal, toggleAgentNavigation, setWindowMode, windowMode]);
}

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
