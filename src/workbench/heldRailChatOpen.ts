import { useEffect } from "react";

import { chatPlacement, type ChatPlacement } from "../app/openChat";
import { useWorkbenchStore } from "../app/store";

/** One caller's chat open: how to open, and whether a held one still applies. */
export interface ChatOpen {
  open: (projectId: string, chatId: string) => Promise<void>;
  /**
   * Whether a held open still applies once the transition settles. By default
   * only a chat known live at the click and archived during the hold is
   * dropped; the open reports the rest.
   */
  admit?: (now: ChatPlacement, wasLive: boolean) => boolean;
  /** Reports a held open refused by a failed transition or by `admit`. */
  refused?: () => void;
}
type HeldChat = {
  projectId: string;
  chatId: string;
  live: boolean;
  request: ChatOpen;
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

const keepUnlessNewlyArchived = (now: ChatPlacement, wasLive: boolean) =>
  !now.archived || !wasLive;

function replayHeld() {
  const state = useWorkbenchStore.getState();
  if (!held || inFlight > 0 || state.projectTransitioning) return;
  const { projectId, chatId, live, request } = held;
  // Clear before opening so a refused replay cannot loop.
  cancelHeldRailChat();
  // A failed transition keeps its error; a replay would start another and
  // clear it.
  if (
    state.error ||
    state.phase !== "ready" ||
    !(request.admit ?? keepUnlessNewlyArchived)(
      chatPlacement(state, projectId, chatId),
      live,
    )
  )
    request.refused?.();
  else void openChatWhenSettled(projectId, chatId, request);
}

export async function openChatWhenSettled(
  projectId: string,
  chatId: string,
  request: ChatOpen,
) {
  const state = useWorkbenchStore.getState();
  if (state.projectTransitioning) {
    // Rails list bounded sets of live and archived chats, so only a listed
    // live chat is known not to be archived already.
    const { attached, archived } = chatPlacement(state, projectId, chatId);
    held = { projectId, chatId, live: attached && !archived, request };
    document.addEventListener("click", cancelOnOtherClick, true);
    if (!watching) {
      watching = true;
      useWorkbenchStore.subscribe((next, previous) => {
        // A failed switch sets its error only after its recovery settles the
        // transition, so the replay waits for that task's microtasks to run.
        if (previous.projectTransitioning && !next.projectTransitioning)
          setTimeout(replayHeld);
      });
    }
    return;
  }
  cancelHeldRailChat();
  inFlight += 1;
  try {
    await request.open(projectId, chatId);
  } finally {
    inFlight -= 1;
  }
  replayHeld();
}

// A held rail click applies only to a chat still listed and not archived.
const railOpen: ChatOpen = {
  open: (project, chat) =>
    useWorkbenchStore.getState().openRailChat(project, chat),
  admit: ({ attached, archived }) => attached && !archived,
};

export const openRailChat = (projectId: string, chatId: string) =>
  openChatWhenSettled(projectId, chatId, railOpen);

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
