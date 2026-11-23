import { useCallback, useEffect, useState } from "react";

import { usePortalHost } from "../ui/usePortalHost";

import { useWorkbenchStore } from "../app/store";
import type { UserTerminalScope } from "../services/userTerminalDesktop";
import { TerminalDockPanel } from "./TerminalDockPanel";
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
  const [[scope, uiProjectGeneration], setOwner] = useState(
    () =>
      [
        selectedScope(pinnedChatId),
        useWorkbenchStore.getState().projectGeneration,
      ] as const,
  );
  if (visible && !scope) {
    const next = selectedScope(pinnedChatId);
    if (next) setOwner([next, useWorkbenchStore.getState().projectGeneration]);
  }
  useEffect(() => {
    if (!scope) return;
    return useWorkbenchStore.subscribe(() => {
      const current = selectedScope(pinnedChatId, scope.chatId);
      if (
        !current ||
        useWorkbenchStore.getState().projectGeneration !==
          uiProjectGeneration ||
        current.projectId !== scope.projectId ||
        current.projectGeneration !== scope.projectGeneration ||
        current.chatId !== scope.chatId
      ) {
        setOwner([null, uiProjectGeneration]);
        onClose();
      }
    });
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

