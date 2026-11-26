import { useEffect } from "react";

import { chatPlacement, openNotifiedChat } from "../app/openChat";
import { useWorkbenchStore } from "../app/store";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";

type Open = (projectId: string, chatId: string) => Promise<void>;
type HeldChat = {
  projectId: string;
  chatId: string;
  archived: boolean;
  open: Open;
};

/**
 * A chat open requested while a transition holds the store (a rail chat click
 * or a notification click) waits here, one slot shared by every caller, latest
 * wins. It opens once the transition ends and no such open is still in flight,
 * so it lands after the open that started the transition. Any later click
 * outside a rail chat row (a project row, New chat, the mode toggle, settings)
 * and the rail unmounting cancel it, so the user's latest action wins. Lives in
 * the lazy rail chunk; startup code never loads it.
 */
let held: HeldChat | null = null;
let inFlight = 0;
let mountedRails = 0;
let watching = false;

// Runs in the capture phase, before the clicked control's own handler.
function cancelOnOtherClick(event: MouseEvent) {
  if (!(event.target as Element | null)?.closest?.("button.project-chat"))
    cancelHeldRailChat();
}

export function cancelHeldRailChat() {
  held = null;
  document.removeEventListener("click", cancelOnOtherClick, true);
}

function replayHeld() {
  const state = useWorkbenchStore.getState();
  if (!held || inFlight > 0 || state.projectTransitioning) return;
  const { projectId, chatId, open } = held;
  // A chat held while archived (a notification restores it) must still be
  // archived; any other held chat must still be attached and not archived.
  const wasArchived = held.archived;
  // Clear before opening so a refused replay cannot loop.
  cancelHeldRailChat();
  // A failed transition keeps its error; a replay would start another and
  // clear it.
  if (state.error || state.phase !== "ready") return;
  const { attached, archived } = chatPlacement(state, projectId, chatId);
  if (archived ? wasArchived : attached)
    void openWhenSettled(projectId, chatId, open);
}

async function openWhenSettled(projectId: string, chatId: string, open: Open) {
  const state = useWorkbenchStore.getState();
  if (state.projectTransitioning) {
    const { archived } = chatPlacement(state, projectId, chatId);
    held = { projectId, chatId, archived, open };
    document.addEventListener("click", cancelOnOtherClick, true);
    if (!watching) {
      watching = true;
      useWorkbenchStore.subscribe((next, previous) => {
        if (previous.projectTransitioning && !next.projectTransitioning)
          queueMicrotask(replayHeld);
      });
    }
    return;
  }
  cancelHeldRailChat();
  inFlight += 1;
  try {
    await open(projectId, chatId);
  } finally {
    inFlight -= 1;
  }
  replayHeld();
}

export const openRailChat = (projectId: string, chatId: string) =>
  openWhenSettled(projectId, chatId, (project, chat) =>
    useWorkbenchStore.getState().openRailChat(project, chat),
  );

/** Opens a clicked notification's chat in Agent, or reports that it is gone. */
export const openNotificationChat = (
  projectId: string,
  chatId: string,
  enterAgent: () => void,
) =>
  openWhenSettled(projectId, chatId, async (project, chat) => {
    await openNotifiedChat(project, chat).catch(() => {});
    const current = useWorkbenchStore.getState();
    if (current.project?.id !== project || current.activeTabId !== chat) {
      useWorkbenchStore.setState({
        error: "The notification chat could not be opened.",
      });
      return;
    }
    useAgentSettingsState.getState().close();
    enterAgent();
  });

/** Drops a held click once no rail is mounted (for example in Editor mode). */
export function useHeldRailChatScope() {
  useEffect(() => {
    mountedRails += 1;
    return () => {
      mountedRails -= 1;
      if (mountedRails === 0) cancelHeldRailChat();
    };
  }, []);
}
