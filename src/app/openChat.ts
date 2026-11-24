import { chatPlacement } from "./chatPlacement";
import { useWorkbenchStore } from "./store";

export { chatPlacement };

export async function openChat(
  projectId: string,
  chatId: string,
  { attached, revealRestored }: { attached: boolean; revealRestored: boolean },
) {
  const store = useWorkbenchStore.getState();
  if (!attached) {
    const restored = await store.restoreChat(chatId, projectId);
    if (!restored || !revealRestored) return;
    if (useWorkbenchStore.getState().project?.id === projectId) return;
  }
  if (projectId === useWorkbenchStore.getState().project?.id) {
    await useWorkbenchStore.getState().activateChat(chatId);
  } else {
    await useWorkbenchStore.getState().openRailChat(projectId, chatId);
  }
}

/**
 * Rails list a bounded set of attached chats, so only a chat known to be closed
 * or archived is restored.
 */
export async function openNotifiedChat(projectId: string, chatId: string) {
  const { archived } = chatPlacement(
    useWorkbenchStore.getState(),
    projectId,
    chatId,
  );
  await openChat(projectId, chatId, {
    attached: !archived,
    revealRestored: true,
  });
}
