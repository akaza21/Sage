import { useId, useRef, useState } from "react";
import "./fork-chat.css";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
import { getGoalChatPage } from "../../services/acpGoalDesktop";
import { ModalDialog } from "../../ui/ModalDialog";
import { FileRewindPreview, useFileRewindPreview } from "./FileRewindPreview";
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
  const [last, setLast] = useState(false);
  // A failed file rewind may already have restored files: asking the native
  // side again keeps the files choice true to what is left to restore.
  const [asked, setAsked] = useState(0);
  // The preview, `null` when files stay as they are, or why it failed.
  const found = useFileRewindPreview(chatId, turnId, asked);
  const files = typeof found === "object" ? found : undefined;
  const rewind = async (withFiles: boolean) => {
    if (pending.current) return;
    pending.current = true;
    setLast(withFiles);
    const context = currentContext(chatId);
    const owner = useWorkbenchStore.getState().captureChatOwnership(chatId);
    try {
      if (
        !(await run(
          {
            kind: "rewindConversation",
            before: turnId,
            ...(withFiles ? { files: true } : {}),
          },
          // A file rewind always reports what it restored.
          "Conversation rewound; files unchanged",
        ))
      ) {
        if (withFiles) setAsked((count) => count + 1);
        return;
      }
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
  const restorable = !!files?.caveat;
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
      <p>Remove this message and later replies from the conversation.</p>
      <FileRewindPreview found={found} />
      {found !== undefined && !files ? <p>Files stay as they are.</p> : null}
      {restorable ? (
        <button
          type="button"
          className="button--compact"
          disabled={busy}
          onClick={() => void rewind(true)}
        >
          Rewind files and conversation
        </button>
      ) : null}
      <button
        type="button"
        className="button--compact"
        autoFocus
        disabled={busy}
        onClick={() => void rewind(false)}
      >
        {files?.fileCount ? "Rewind conversation only" : "Confirm rewind"}
      </button>
      {status.kind === "failed" ? (
        <p role="status">
          {status.message}{" "}
          <button type="button" onClick={() => void rewind(last && restorable)}>
            Retry
          </button>
        </p>
      ) : null}
    </ModalDialog>
  );
}
