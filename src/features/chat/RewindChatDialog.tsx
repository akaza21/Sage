import { useId, useRef } from "react";
import "./fork-chat.css";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
import { getGoalChatPage } from "../../services/acpGoalDesktop";
import { ModalDialog } from "../../ui/ModalDialog";
import { currentContext } from "./acp/useAcpChatActions";
import { useSessionControl } from "./acp/useSessionControl";
import type { RewindChatRequest } from "./forkChatRequest";

/** Hosted outside the source turn so recovery and virtualization cannot lose Retry. */
export function RewindChatDialog({
  chatId,
  turnId,
  text,
  onClose,
}: RewindChatRequest & { onClose: () => void }) {
  const title = useId();
  const { status, run } = useSessionControl(chatId);
  const pending = useRef(false);
  const rewind = async () => {
    if (pending.current) return;
    pending.current = true;
    const context = currentContext(chatId);
    const owner = useWorkbenchStore.getState().captureChatOwnership(chatId);
    try {
      if (
        !(await run(
          { kind: "rewindConversation", before: turnId },
          "Conversation rewound; files unchanged",
        ))
      )
        return;
      if (owner) useWorkbenchStore.getState().setChatComposer(owner, text);
      if (context) {
        const refreshed = await getGoalChatPage({
          ...context.scope,
          projectGeneration: context.commandProjectGeneration,
          cursor: null,
        }).catch(() => null);
        if (refreshed)
          acpProjectionStoreFor(chatId)
            .getState()
            .applyAuthoritativePage(context.scope, refreshed);
      }
      onClose();
    } finally {
      pending.current = false;
    }
  };
  const busy = status.kind === "pending";
  return (
    <ModalDialog
      className="fork-chat-dialog dialog--scrim"
      labelledBy={title}
      restoreFocus
      onDismiss={() => {
        if (!pending.current) onClose();
      }}
    >
      <h2 id={title}>Rewind conversation from here</h2>
      <p>
        Remove this message and later replies from the conversation. Files stay
        as they are.
      </p>
      <button
        type="button"
        className="button--compact"
        autoFocus
        disabled={busy}
        onClick={() => void rewind()}
      >
        Confirm rewind
      </button>
      {status.kind === "failed" ? (
        <p role="status">
          {status.message}{" "}
          <button type="button" onClick={() => void rewind()}>
            Retry
          </button>
        </p>
      ) : null}
    </ModalDialog>
  );
}
