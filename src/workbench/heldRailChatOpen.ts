import { useWorkbenchStore, type WorkbenchState } from "../app/store";

type HeldChat = { projectId: string; chatId: string };

/**
 * A rail chat clicked while a transition holds the store waits here, one slot
 * shared by every rail row, latest wins. It opens once the transition ends and
 * no rail open is still in flight, so it lands after the open that started the
 * transition. Lives in the lazy rail chunk; startup code never loads it.
 */
let held: HeldChat | null = null;
let inFlight = 0;
let watching = false;

function stillOpen(state: WorkbenchState, { projectId, chatId }: HeldChat) {
  if (state.project?.id === projectId)
    return state.tabs.some((tab) => tab.id === chatId && tab.kind === "chat");
  const rail = [...state.projects, ...state.unscopedChats].find(
    (item) => item.id === projectId,
  );
  return Boolean(
    rail?.railChats?.some((chat) => chat.id === chatId) &&
    !rail.railArchived?.some((chat) => chat.id === chatId),
  );
}

function replayHeld() {
  const state = useWorkbenchStore.getState();
  if (!held || inFlight > 0 || state.projectTransitioning) return;
  const target = held;
  // Clear before opening so a refused replay cannot loop.
  held = null;
  if (stillOpen(state, target))
    void openRailChat(target.projectId, target.chatId);
}

export async function openRailChat(projectId: string, chatId: string) {
  const state = useWorkbenchStore.getState();
  if (state.projectTransitioning) {
    held = { projectId, chatId };
    if (!watching) {
      watching = true;
      useWorkbenchStore.subscribe((next, previous) => {
        if (previous.projectTransitioning && !next.projectTransitioning)
          queueMicrotask(replayHeld);
      });
    }
    return;
  }
  held = null;
  inFlight += 1;
  try {
    await state.openRailChat(projectId, chatId);
  } finally {
    inFlight -= 1;
  }
  replayHeld();
}
