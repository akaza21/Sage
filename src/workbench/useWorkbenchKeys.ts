import {
  useCallback,
  useEffect,
  useRef,
  type Dispatch,
  type SetStateAction,
} from "react";

import { useTerminalDockStore } from "./workbenchLayoutStorage";
import { isTauriRuntime } from "../ports/nativeInvoke";
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

/** One command for the status button, launchers and keyboard; native settings own visibility. */
export function useTerminalDock(blocked: boolean) {
  const visible = useTerminalDockStore((state) => state.visible);
  const ready = useTerminalDockStore((state) => state.ready);
  const error = useTerminalDockStore((state) => state.error);
  const returnFocus = useRef<HTMLElement | null>(null);
  const available = isTauriRuntime() && ready;
  const setOpen = useCallback((open: boolean) => {
    const state = useTerminalDockStore.getState();
    if (!state.ready || state.visible === open) return;
    if (open) {
      returnFocus.current =
        document.activeElement instanceof HTMLElement
          ? document.activeElement
          : null;
    }
    state.setVisible(open);
    if (!open) {
      const target = returnFocus.current;
      const valid =
        target?.isConnected &&
        target !== document.body &&
        !target.closest(".terminal-dock");
      if (valid) target.focus();
      if (!valid || document.activeElement !== target)
        document
          .querySelector<HTMLButtonElement>(".status-bar__terminal")
          ?.focus();
    }
  }, []);
  const open = useCallback(() => setOpen(true), [setOpen]);
  const toggle = useCallback(() => {
    setOpen(!useTerminalDockStore.getState().visible);
  }, [setOpen]);
  useEffect(() => {
    void useTerminalDockStore.getState().restore();
  }, []);
  useEffect(() => {
    const keyDown = (event: KeyboardEvent) => {
      if (
        !available ||
        blocked ||
        event.defaultPrevented ||
        event.isComposing ||
        document.querySelector("dialog[open]")
      )
        return;
      const chord = matchesPlatformChord(
        keymapBinding("agent.terminal"),
        event,
        usesMetaModifier(),
      );
      const escape =
        event.key === "Escape" &&
        !event.ctrlKey &&
        !event.metaKey &&
        !event.altKey &&
        !event.shiftKey &&
        event.target instanceof Element &&
        event.target.closest(".terminal-dock");
      if (!chord && !escape) return;
      if (event.repeat) return;
      event.preventDefault();
      event.stopPropagation();
      if (chord) toggle();
      else setOpen(false);
    };
    // Capture before xterm translates the shortcut or Escape into PTY input.
    window.addEventListener("keydown", keyDown, true);
    return () => window.removeEventListener("keydown", keyDown, true);
  }, [available, blocked, toggle, setOpen]);
  return {
    visible: isTauriRuntime() && visible,
    available,
    error,
    open,
    toggle,
    setOpen,
  };
}
