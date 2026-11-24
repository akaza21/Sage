import { useEffect } from "react";

import { chatPlacement } from "../app/openChat";
import { useWorkbenchStore } from "../app/store";

type HeldChat = { projectId: string; chatId: string };

/**
 * A rail chat clicked while a transition holds the store waits here, one slot
 * shared by every rail row, latest wins. It opens once the transition ends and
 * no rail open is still in flight, so it lands after the open that started the
 * transition. Any later click outside a rail chat row (a project row, New chat,
 * the mode toggle, settings) and the rail unmounting cancel it, so the user's
 * latest action wins. Lives in the lazy rail chunk; startup code never loads it.
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
  const { projectId, chatId } = held;
  // Clear before opening so a refused replay cannot loop.
  cancelHeldRailChat();
  // A failed transition keeps its error; a replay would start another and
  // clear it.
  if (state.error || state.phase !== "ready") return;
  const { attached, archived } = chatPlacement(state, projectId, chatId);
  if (attached && !archived) void openRailChat(projectId, chatId);
}

export async function openRailChat(projectId: string, chatId: string) {
  const state = useWorkbenchStore.getState();
  if (state.projectTransitioning) {
    held = { projectId, chatId };
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
    await state.openRailChat(projectId, chatId);
  } finally {
    inFlight -= 1;
  }
  replayHeld();
}

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
