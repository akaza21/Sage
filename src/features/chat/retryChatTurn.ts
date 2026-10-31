import { useWorkbenchStore } from "../../app/store";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import type {
  AcpChatPage,
  CuratedBindingId,
  CuratedModelId,
} from "../../domain/acp";
import type { AttachedContext } from "../../domain/attachedContext";
import type { AcpActionOutcome } from "./acp/acpActionController";
import { latestRetryPrompt, promptAttachmentItems } from "./acp/acpTurnState";

export async function retryChatTurn(
  page: AcpChatPage,
  submit: (
    prompt: string,
    context: readonly AttachedContext[],
    bindingId: CuratedBindingId,
    modelId: CuratedModelId | null,
  ) => Promise<AcpActionOutcome>,
) {
  const retry = latestRetryPrompt(page);
  if (!retry) return;
  const state = useWorkbenchStore.getState();
  const authoritative = acpProjectionStoreFor(page.chatId).getState().page;
  if (
    authoritative?.currentSessionId !== page.currentSessionId ||
    authoritative.projectionRevision !== page.projectionRevision ||
    authoritative.turns.at(-1)?.id !== page.turns.at(-1)?.id
  )
    return;
  const intent = state.captureChatOwnership(page.chatId);
  if (!intent || state.project?.id !== page.projectId) return;
  const unavailable =
    promptAttachmentItems(page.turns.at(-1)!.items).length >
    retry.attachedContext.filter((context) => context.kind === "image").length;
  const outcome = unavailable
    ? "rejected"
    : await submit(
        retry.prompt,
        retry.attachedContext,
        retry.bindingId,
        retry.modelId,
      );
  if (outcome !== "rejected" || (!unavailable && !retry.attachedContext.length))
    return;
  const current = useWorkbenchStore.getState();
  if (
    acpProjectionStoreFor(page.chatId).getState().page?.currentSessionId !==
    page.currentSessionId
  )
    return;
  const reason = current.chatUiById[page.chatId]?.error;
  const names = retry.attachedContext
    .map((context) => (context.kind === "image" ? context.label : context.kind))
    .join(", ");
  current.setChatActionState(intent, {
    sending: false,
    error: `${reason ?? "Saved attachment validation failed."} Reattach unavailable items (${names || "saved image"}) in the composer and send again. Retry did not send without them.`,
  });
}
