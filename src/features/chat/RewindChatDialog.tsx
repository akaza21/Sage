import { useEffect, useId, useRef, useState } from "react";
import "./fork-chat.css";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
import { invokeAcp } from "../../services/acpDesktopCommand";
import { isRecord } from "../../services/acpDesktopGuards";
import { getGoalChatPage } from "../../services/acpGoalDesktop";
import { ModalDialog } from "../../ui/ModalDialog";
import { currentContext } from "./acp/useAcpChatActions";
import { useSessionControl } from "./acp/useSessionControl";
import type { RewindChatRequest } from "./forkChatRequest";

/** The native dry run's lists, each at most 200 paths with counts for the rest. */
type Preview = {
  files: string[];
  fileCount: number;
  conflicts: string[];
  conflictCount: number;
};

const list = (value: unknown) =>
  Array.isArray(value) &&
  value.length <= 200 &&
  value.every((path) => typeof path === "string");

/** A dry run only: no file changes until the user confirms. `null` means files stay. */
async function preview(chatId: string, before: string) {
  const context = currentContext(chatId)!;
  const session = context.page.sessions.find(
    (item) => item.id === context.page.currentSessionId,
  )!;
  const value = await invokeAcp<{ preview?: unknown }, object>(
    "control_acp_session",
    {
      ...context.scope,
      projectGeneration: context.commandProjectGeneration,
      sessionId: session.id,
      runtimeEpoch: session.runtimeEpoch,
      control: { kind: "previewRewind", before },
    },
  );
  const found = value?.preview;
  if (found === undefined) return null;
  if (
    !isRecord(found) ||
    !list(found.files) ||
    !list(found.conflicts) ||
    !Number.isSafeInteger(found.fileCount) ||
    !Number.isSafeInteger(found.conflictCount)
  )
    throw new Error();
  return found as Preview;
}

function Paths({ paths, count }: { paths: string[]; count: number }) {
  return (
    <ul className="rewind-files">
      {paths.map((path) => (
        <li key={path}>{path}</li>
      ))}
      {count > paths.length ? <li>+{count - paths.length} more</li> : null}
    </ul>
  );
}

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
  const [files, setFiles] = useState<Preview | null | undefined>();
  const [last, setLast] = useState(false);
  useEffect(() => {
    let live = true;
    preview(chatId, turnId).then(
      (next) => live && setFiles(next),
      () => live && setFiles(null),
    );
    return () => {
      live = false;
    };
  }, [chatId, turnId]);
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
          withFiles
            ? "Files restored; conversation rewound"
            : "Conversation rewound; files unchanged",
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
  const busy = status.kind === "pending" || files === undefined;
  const restorable = !!files?.fileCount && !files.conflictCount;
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
      {files?.conflictCount ? (
        <>
          <p role="alert">
            Files changed since the agent last wrote them, so they stay:
          </p>
          <Paths paths={files.conflicts} count={files.conflictCount} />
        </>
      ) : null}
      {restorable ? (
        <>
          <p>These files go back to before this message:</p>
          <Paths paths={files.files} count={files.fileCount} />
          <p>
            <small>
              Rewind does not undo shell commands, commits, branch switches, or
              files changed outside checkpoints.
            </small>
          </p>
          <button
            type="button"
            className="button--compact"
            autoFocus
            disabled={busy}
            onClick={() => void rewind(true)}
          >
            Rewind files and conversation
          </button>
        </>
      ) : null}
      <button
        type="button"
        className="button--compact"
        autoFocus={!restorable}
        disabled={busy}
        onClick={() => void rewind(false)}
      >
        {files?.fileCount ? "Rewind conversation only" : "Confirm rewind"}
      </button>
      {status.kind === "failed" ? (
        <p role="status">
          {status.message}{" "}
          <button type="button" onClick={() => void rewind(last)}>
            Retry
          </button>
        </p>
      ) : null}
    </ModalDialog>
  );
}
