import { useWorkbenchStore } from "../../app/store";
import type { ImageThumbnailOwner } from "./ImageAttachmentThumbnail";

export function useImageThumbnailOwner(
  chatId?: string,
): ImageThumbnailOwner | undefined {
  const projectId = useWorkbenchStore((state) => state.project?.id);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const nativeTransitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const ownerChatId = chatId ?? activeTabId;
  if (!projectId || !ownerChatId) return undefined;
  return {
    projectId,
    projectGeneration: String(nativeTransitionEpoch),
    chatId: ownerChatId,
  };
}
