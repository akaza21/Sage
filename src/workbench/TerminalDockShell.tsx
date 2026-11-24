import { useCallback, useEffect, useState } from "react";

import { usePortalHost } from "../ui/usePortalHost";

import { useWorkbenchStore } from "../app/store";
import { useTerminalDockStore } from "../app/terminalDockStore";
import type { UserTerminalScope } from "../services/userTerminalDesktop";
import { TerminalDockPanel } from "./TerminalDockPanel";
import { forScope } from "./terminalScope";
import { firstChatTabId } from "./workbenchWindowMode";
import "./terminal-dock.css";

function selectedScope(
  pinnedChatId: string | null,
  preferredChatId?: string,
): UserTerminalScope | null {
  const state = useWorkbenchStore.getState();
  if (!state.project || state.projectTransitioning) return null;
  const chatId =
    [state.activeTabId, preferredChatId, pinnedChatId].find((id) =>
      state.tabs.some((tab) => tab.kind === "chat" && tab.id === id),
    ) ?? firstChatTabId(state.tabs);
  if (!chatId) return null;
  return {
    projectId: state.project.id,
    projectGeneration: String(state.nativeTransitionEpoch),
    chatId,
  };
}

type WorkbenchDockProps = {
  inert?: boolean;
  setOpen: (open: boolean) => void;
  pinnedChatId: string | null;
  visible: boolean;
  open: boolean;
};

export default function TerminalDockForWorkbench({
  container,
  inert,
  setOpen,
  pinnedChatId,
  visible,
  open,
}: WorkbenchDockProps & { container: HTMLElement | null }) {
  const { portal } = usePortalHost(container);
  const onClose = useCallback(() => setOpen(false), [setOpen]);
  // The dock takes its chat's scope when shown (below), and again after a
  // project or chat change released it.
  const [[scope, uiProjectGeneration], setOwner] = useState<
    readonly [UserTerminalScope | null, number]
  >([null, 0]);
  if (visible && !scope) {
    const next = selectedScope(pinnedChatId);
    if (next) setOwner([next, useWorkbenchStore.getState().projectGeneration]);
  }
  useEffect(() => {
    if (!scope) return;
    useTerminalDockStore.setState({ owner: scope.chatId });
    const unsubscribe = useWorkbenchStore.subscribe(() => {
      const current = selectedScope(pinnedChatId, scope.chatId);
      if (
        !forScope(current, scope) ||
        useWorkbenchStore.getState().projectGeneration !== uiProjectGeneration
      ) {
        setOwner([null, uiProjectGeneration]);
        onClose();
      }
    });
    return () => {
      unsubscribe();
      useTerminalDockStore.setState({ owner: null });
    };
  }, [scope, onClose, pinnedChatId, uiProjectGeneration]);
  return portal(
    scope ? (
      <TerminalDockPanel
        scope={scope}
        onClose={onClose}
        inert={inert}
        visible={visible}
        open={open}
      />
    ) : visible ? (
      <section className="terminal-dock" aria-label="Terminal" inert={inert}>
        <div className="terminal-dock__header">Terminal</div>
        <p className="terminal-dock__message" role="status">
          Open a chat first.
        </p>
        <button type="button" onClick={onClose}>
          Close terminal
        </button>
      </section>
    ) : null,
  );
}
