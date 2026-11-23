import { useEffect, useId, useRef, useState } from "react";
import "./fork-chat.css";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
import { AcpDesktopError, invokeAcp } from "../../services/acpDesktopCommand";
import { isRecord } from "../../services/acpDesktopGuards";
import { getGoalChatPage } from "../../services/acpGoalDesktop";
import { ModalDialog } from "../../ui/ModalDialog";
import { currentContext } from "./acp/useAcpChatActions";
import { sessionRequest, useSessionControl } from "./acp/useSessionControl";
import type { RewindChatRequest } from "./forkChatRequest";

/** At most 200 paths, with the count of all of them. */
type Listed = { paths: string[]; count: number };
/**
 * The native dry run: the files it keeps with why, or else the files it would
 * restore. `caveat` (what rewind does not undo) comes exactly when it restores.
 */
type Preview = {
  fileCount: number;
  sections: (Listed & { note: string; keeps?: true })[];
  caveat: string | null;
};

const listed = (
  value: Record<string, unknown>,
  paths: string,
  count: string,
) => {
  const list = value[paths];
  return (
    Array.isArray(list) &&
    list.length <= 200 &&
    list.every((path) => typeof path === "string") &&
    Number.isSafeInteger(value[count])
  );
};

/** A dry run only: no file changes until the user confirms. `null` means files stay. */
async function preview(chatId: string, before: string) {
  const request = sessionRequest(chatId, { kind: "previewRewind", before });
  if (!request) return null;
  const value = await invokeAcp<{ preview?: unknown }, object>(
    "control_acp_session",
    request,
  );
  const found = value?.preview;
  if (found === undefined) return null;
  if (
    !isRecord(found) ||
    !Number.isSafeInteger(found.fileCount) ||
    !(found.caveat === null || typeof found.caveat === "string") ||
    !Array.isArray(found.sections) ||
    found.sections.length > 2 ||
    !found.sections.every(
      (section) =>
        isRecord(section) &&
        typeof section.note === "string" &&
        listed(section, "paths", "count"),
    )
  )
    throw new Error();
  return found as Preview;
}

function Paths({ paths, count }: Listed) {
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
  // The preview, or why there is none: files then stay as they are.
  const [found, setFound] = useState<Preview | string>();
  const files = typeof found === "object" ? found : undefined;
  const [last, setLast] = useState(false);
  // A failed file rewind may already have restored files: asking the native
  // side again keeps the files choice true to what is left to restore.
  const [asked, setAsked] = useState(0);
  useEffect(() => {
    let live = true;
    preview(chatId, turnId).then(
      (next) => live && setFound(next ?? ""),
      (error: unknown) =>
        live && setFound(error instanceof AcpDesktopError ? error.message : ""),
    );
    return () => {
      live = false;
    };
  }, [chatId, turnId, asked]);
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
        if (withFiles) {
          setFound(undefined);
          setAsked((count) => count + 1);
        }
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
      {typeof found === "string" ? (
        <p>{found} Files stay as they are.</p>
      ) : null}
      {files?.sections.map((section) => (
        <div key={section.note}>
          <p role={section.keeps && "alert"}>{section.note}</p>
          <Paths {...section} />
        </div>
      ))}
      {restorable ? (
        <>
          <p>
            <small>{files.caveat}</small>
          </p>
          <button
            type="button"
            className="button--compact"
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
