import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from "react";

import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
import { isTauriRuntime } from "../../ports/nativeInvoke";
import { nativePromptAttachments as port } from "../../ports/nativePromptAttachments";
import type {
  PromptFileOwner,
  StagedPromptFiles,
} from "../../ports/promptAttachments";
import { MenuPopover } from "../../ui/MenuPopover";
import { FileText, X } from "../../ui/leanIcons";

/** Sizes as people read them: whole kilobytes, rounded up. */
function formatSize(bytes: number): string {
  return `${Math.max(1, Math.ceil(bytes / 1024)).toLocaleString("en-US")} KB`;
}

function failure(error: unknown): string {
  if (typeof error === "string") return error;
  return error instanceof Error
    ? error.message
    : "The attachments could not be updated.";
}

/** The chat's own owner; never another tab's, such as the active one. */
function owner(chatId: string): PromptFileOwner | null {
  const intent = useWorkbenchStore.getState().captureChatOwnership(chatId);
  return intent && isTauriRuntime()
    ? {
        projectId: intent.projectId,
        chatId: intent.chatId,
        projectGeneration: intent.transitionEpoch,
      }
    : null;
}

/**
 * Why files cannot be staged for this chat's next message, or null. Native
 * admission treats an absent capability as unsupported; a chat without a
 * session yet is checked again when its first message starts one.
 */
function refusal(chatId: string): string | null {
  if (!isTauriRuntime())
    return "Attaching files needs the Sage desktop app with an ACP harness.";
  const page = acpProjectionStoreFor(chatId).getState().page;
  const session = page?.sessions.find(
    (item) => item.id === page.currentSessionId,
  );
  return session && session.capabilities.promptResources !== true
    ? "This harness does not accept attached files (no embedded-context support)."
    : null;
}

/**
 * Lazy: loads with a desktop chat's composer, so a batch native already
 * holds shows its chips, or elsewhere with the first attach. Picks files in the native
 * dialog, which stages them in Rust (the WebView never sees a path or the
 * bytes), shows the chat's staged files as chips and removes them. Images
 * go through the composer's image path when the harness takes images.
 */
export function ComposerAttach({
  chatId,
  presses,
  triggerId,
  imageAction,
  disabled,
}: {
  chatId: string;
  /** Presses of the attach button so far; each is taken once. */
  presses: number;
  /** The attach button, which anchors the menu and takes the focus back. */
  triggerId: string;
  imageAction?: { disabled: boolean; onChoose: () => void };
  /** Busy while a send is in flight; a send may consume the batch. */
  disabled: boolean;
}) {
  const [staged, setStaged] = useState<StagedPromptFiles | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [menuOpen, setMenuOpen] = useState(false);
  const revision = useRef<string | null>(null);
  const menuId = useId();
  // The composer keeps this part across chats: another chat starts empty
  // and reads its own batch back.
  const [scope, setScope] = useState(chatId);
  if (scope !== chatId) {
    setScope(chatId);
    setStaged(null);
    setNotice("");
    setMenuOpen(false);
  }
  const current = useRef(chatId);
  useLayoutEffect(() => {
    current.current = chatId;
    revision.current = null;
  }, [chatId]);
  // Native calls run one at a time, in order, so a pick never races the
  // read-back of the batch it replaces. A result for a chat left meanwhile
  // is dropped; that chat reads its batch back when shown again.
  const chain = useRef(Promise.resolve());
  const run = useCallback(
    (
      change: (
        target: PromptFileOwner,
      ) => Promise<StagedPromptFiles | null | undefined>,
      done?: (next: StagedPromptFiles | null) => string,
    ) => {
      chain.current = chain.current.then(async () => {
        const target = owner(chatId);
        // Only an action the person took says why it could not run.
        if (!target) return done && setNotice(refusal(chatId) ?? "");
        setBusy(true);
        try {
          const next = await change(target);
          if (next === undefined || current.current !== target.chatId) return;
          revision.current = next?.revision ?? null;
          setStaged(next);
          if (done) setNotice(done(next));
        } catch (error) {
          setNotice(failure(error));
        } finally {
          setBusy(false);
        }
      });
    },
    [chatId],
  );
  // Native truth on load, on a chat change and after each send, which
  // consumes the batch it carried.
  useEffect(() => {
    if (!disabled) run((target) => port.listPromptFiles(target));
  }, [disabled, run]);
  const pickFiles = useCallback(() => {
    setMenuOpen(false);
    run(
      // A cancelled pick (null) keeps what was staged.
      async (target) =>
        (await port.stagePromptFiles({
          owner: target,
          pick: "files",
          expectedRevision: revision.current,
        })) ?? undefined,
      (next) => {
        const count = next?.files.length ?? 0;
        return `${count} ${count === 1 ? "file" : "files"} attached to the next message.`;
      },
    );
  }, [run]);
  // Each press is taken once: it opens the menu, says why files cannot be
  // attached, or asks for the native picker, which opens once per ask.
  const [taken, setTaken] = useState(0);
  const [picks, setPicks] = useState(0);
  if (presses !== taken) {
    setTaken(presses);
    const refused = refusal(chatId);
    if (imageAction) setMenuOpen(true);
    else if (refused) setNotice(refused);
    else setPicks((count) => count + 1);
  }
  const picked = useRef(0);
  useEffect(() => {
    if (picks === picked.current) return;
    picked.current = picks;
    pickFiles();
  }, [pickFiles, picks]);
  // The menu and the focus after a removal anchor on the attach button.
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  useLayoutEffect(() => {
    triggerRef.current = document.getElementById(
      triggerId,
    ) as HTMLButtonElement | null;
  });
  const files = staged?.files ?? [];
  const refused = menuOpen ? refusal(chatId) : null;
  return (
    <>
      <p className="sr-only" role="status" aria-live="polite">
        {notice}
      </p>
      {files.length > 0 ? (
        <div className="composer__attachment-row">
          <ul
            className="composer__attachments"
            aria-label="Files for the next message"
          >
            {files.map((file) => {
              const label = `${file.name} · ${formatSize(file.bytes)}`;
              return (
                <li className="composer__attachment" key={file.id}>
                  <FileText size={12} aria-hidden="true" />
                  <span>{label}</span>
                  <button
                    className="composer__attachment-remove"
                    type="button"
                    aria-label={`Remove ${label}`}
                    disabled={disabled || busy}
                    onClick={() => {
                      // The chip is going away; keep focus in the composer.
                      triggerRef.current?.focus();
                      run(
                        (target) =>
                          port.removePromptFile({
                            owner: target,
                            revision: revision.current ?? "",
                            id: file.id,
                          }),
                        () => `Removed ${file.name}.`,
                      );
                    }}
                  >
                    <X size={12} aria-hidden="true" />
                  </button>
                </li>
              );
            })}
          </ul>
        </div>
      ) : null}
      {menuOpen ? (
        <MenuPopover
          id={menuId}
          className="composer__popover"
          ariaLabel="Attach"
          open
          triggerRef={triggerRef}
          onDismiss={() => setMenuOpen(false)}
          gap={8}
        >
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            tabIndex={-1}
            disabled={busy || refused !== null}
            title={refused ?? undefined}
            onClick={pickFiles}
          >
            Files…
          </button>
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            tabIndex={-1}
            disabled={imageAction?.disabled}
            onClick={() => {
              setMenuOpen(false);
              imageAction?.onChoose();
            }}
          >
            Images…
          </button>
        </MenuPopover>
      ) : null}
    </>
  );
}
