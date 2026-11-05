import {
  createElement,
  useEffect,
  useEffectEvent,
  useState,
  useRef,
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

import {
  connectNavigationHistory,
  navigateWorkbenchBack,
  navigateWorkbenchForward,
} from "./navigationHistory";

/** Window-level commands shared by Agent and Editor, guarded by resource modals. */
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
  const modeRef = useRef(windowMode);
  const historyRef = useRef<ReturnType<typeof connectNavigationHistory> | null>(
    null,
  );
  useEffect(() => {
    modeRef.current = windowMode;
    historyRef.current?.observe();
  }, [windowMode]);
  useEffect(() => {
    const history = connectNavigationHistory(
      () => modeRef.current,
      (mode) => {
        modeRef.current = mode;
        setWindowMode(mode);
      },
    );
    historyRef.current = history;
    return () => {
      historyRef.current = null;
      history.dispose();
    };
  }, [setWindowMode]);
  useEffect(() => {
    const onHistoryKey = (event: KeyboardEvent) => {
      if (resourceModal || event.defaultPrevented || event.isComposing) return;
      const apple = usesMetaModifier();
      const back = matchesPlatformChord(
        keymapBinding("workbenchBack"),
        event,
        apple,
      );
      const forward = matchesPlatformChord(
        keymapBinding("workbenchForward"),
        event,
        apple,
      );
      if (!back && !forward) return;
      if (
        event.target instanceof Element &&
        event.target.closest("input, textarea, select, [contenteditable]")
      )
        return;
      event.preventDefault();
      event.stopImmediatePropagation();
      void (back ? navigateWorkbenchBack() : navigateWorkbenchForward());
    };
    window.addEventListener("keydown", onHistoryKey, true);
    return () => window.removeEventListener("keydown", onHistoryKey, true);
  }, [resourceModal]);
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
        if (windowMode === "agent") toggleAgentNavigation();
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
  }, [resourceModal, toggleAgentNavigation, setWindowMode, windowMode]);
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
