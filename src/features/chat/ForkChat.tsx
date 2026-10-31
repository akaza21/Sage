import { useEffect, useId, useRef, useState, type RefObject } from "react";

import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import {
  commitIdentityChatResult,
  createChatTransitionContext,
} from "../../app/chatTransitions";
import { currentProjectLayout } from "../../app/projectProjection";
import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage } from "../../domain/acp";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import { Copy, Ellipsis, GitBranch } from "../../ui/leanIcons";
import { MenuPopover } from "../../ui/MenuPopover";
import { ModalDialog } from "../../ui/ModalDialog";
import { isSideChatId } from "../../workbench/sideChatState";
import "./fork-chat.css";
import { inheritComposerModel } from "./inheritedComposerModel";

/** Lazy fork UI; Rust owns the fork and authoritative Project snapshot. */

/** Offer Branch only when forkable; otherwise reuse the row's Copy feedback. */
export function AnswerMoreMenu({
  onCopy,
  onBranch,
  trigger,
}: {
  onCopy: () => void;
  /** Present only when this answer can be forked; true when keyed. */
  onBranch?: (keyboard: boolean) => void;
  trigger: RefObject<HTMLButtonElement | null>;
}) {
  const menuId = useId();
  const [open, setOpen] = useState(false);
  // Only Enter/Space counts as keyboard activation.
  const keyed = useRef(false);
  const [Icon, label] = onBranch
    ? [GitBranch, "Branch in new chat"]
    : [Copy, "Copy"];
  return (
    <>
      <button
        ref={trigger}
        type="button"
        aria-label="More actions"
        title="More actions"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        onClick={() => setOpen(!open)}
      >
        <Ellipsis size={14} aria-hidden="true" />
      </button>
      <MenuPopover
        id={menuId}
        open={open}
        ariaLabel="Answer actions"
        className="answer-more-menu"
        gap={2}
        placement="above-start"
        triggerRef={trigger}
        onDismiss={() => setOpen(false)}
      >
        <button
          type="button"
          role="menuitem"
          className="button--compact answer-more-menu__item"
          onKeyDown={(event) => {
            keyed.current = event.key === "Enter" || event.key === " ";
          }}
          onClick={() => {
            setOpen(false);
            if (onBranch) onBranch(keyed.current);
            else {
              trigger.current?.focus();
              onCopy();
            }
            keyed.current = false;
          }}
        >
          <Icon size={14} aria-hidden="true" />
          <span>{label}</span>
        </button>
      </MenuPopover>
    </>
  );
}

export interface ForkTarget {
  chatId: string;
  turnId: string;
}

/** Resolves to null on success or to the reason the fork did not commit. */
// eslint-disable-next-line react-refresh/only-export-components -- the chunk's fork action, shared by its parts and tests
export async function forkChat({
  chatId,
  turnId,
}: ForkTarget): Promise<string | null> {
  const { setState: set, getState: get } = useWorkbenchStore;
  const layout = currentProjectLayout(get());
  if (!layout || get().projectTransitioning)
    return "Wait for the current Project operation.";
  const context = createChatTransitionContext(set, get);
  // The native fork keeps the parent's model and effort.
  const parentConfig =
    acpProjectionStoreFor(chatId).getState().page?.sessionConfig;
  set({ projectTransitioning: true, error: null });
  const committed = await commitIdentityChatResult({
    set,
    get,
    applyProjection: context.applyProjection,
    loadAuthoritativeProjection: context.loadAuthoritativeProjection,
    failedMessage: "The fork did not commit.",
    invoke: () => {
      const fork = getWorkbenchPorts().chat.forkChat;
      if (!fork) throw new Error("Forking needs the desktop app.");
      return fork({
        currentLayout: layout,
        chatId,
        turnId,
        expectedTransitionEpoch: get().nativeTransitionEpoch,
      });
    },
  });
  if (committed) {
    // Rust activates the fork; it replaces the parent's pane, unpaired.
    const fork = get().activeTabId;
    if (fork && fork !== chatId) {
      inheritComposerModel(fork, parentConfig);
    }
    return null;
  }
  // The dialog reports the failure inline instead of the global banner.
  const reason = get().error;
  set({ error: null });
  return reason || "Try again.";
}

export function AnswerMoreActions({
  page,
  onCopy,
  forkable,
}: {
  page: AcpChatPage;
  onCopy: () => void;
  /** Only the latest answer's row may fork; earlier and inherited never. */
  forkable?: boolean;
}) {
  const trigger = useRef<HTMLButtonElement>(null);
  // null while closed; otherwise whether Enter or Space opened the dialog.
  const [forking, setForking] = useState<boolean | null>(null);
  const turn = page.turns.at(-1);
  const canFork =
    forkable &&
    turn?.state === "completed" &&
    turn.sessionId === page.currentSessionId &&
    !isSideChatId(page.chatId) &&
    page.sessions.some(
      (session) =>
        session.id === turn.sessionId &&
        session.residency === "hot" &&
        session.capabilities.nativeSessionFork,
    );
  if (!canFork) return null;
  return (
    <>
      <AnswerMoreMenu
        onCopy={onCopy}
        trigger={trigger}
        onBranch={canFork ? setForking : undefined}
      />
      {forking !== null && canFork ? (
        <ForkChatDialog
          chatId={page.chatId}
          turnId={turn.id}
          keyboard={forking}
          onClose={() => {
            setForking(null);
            trigger.current?.focus();
          }}
        />
      ) : null}
    </>
  );
}

/** No worktree fork: Chats lack worktree support; worktrees aren't Projects. */
export function ForkChatDialog({
  chatId,
  turnId,
  onClose,
  keyboard = false,
  fork = forkChat,
}: ForkTarget & {
  onClose: () => void;
  /** Opened with Enter or Space; a pointer or scripted click is not. */
  keyboard?: boolean;
  fork?: (target: ForkTarget) => Promise<string | null>;
}) {
  const title = useId();
  const option = useRef<HTMLButtonElement>(null);
  const busyRef = useRef(false);
  const [result, setResult] = useState<string | null | undefined>(null);
  const busy = result === undefined;
  // Focus the first option; highlight only for keyboard input.
  const [fresh, setFresh] = useState(!keyboard);
  const dismiss = () => {
    if (!busyRef.current) onClose();
  };
  useEffect(() => {
    const dialog = option.current?.closest("dialog");
    if (!dialog) return;
    const backdrop = (event: MouseEvent) => {
      if (event.target !== dialog || busyRef.current) return;
      const box = dialog.getBoundingClientRect();
      if (
        event.clientX < box.left ||
        event.clientX > box.right ||
        event.clientY < box.top ||
        event.clientY > box.bottom
      )
        onClose();
    };
    dialog.addEventListener("click", backdrop);
    return () => dialog.removeEventListener("click", backdrop);
  }, [onClose]);
  return (
    <ModalDialog
      className="fork-chat-dialog dialog--scrim"
      labelledBy={title}
      onDismiss={dismiss}
    >
      <h2 id={title}>Fork chat from here</h2>
      <div
        role="menu"
        aria-labelledby={title}
        aria-busy={busy}
        onKeyDown={() => setFresh(false)}
      >
        <button
          ref={option}
          autoFocus
          className="button--compact"
          type="button"
          role="menuitem"
          data-fresh={fresh || undefined}
          disabled={busy}
          onClick={async () => {
            busyRef.current = true;
            setResult(undefined);
            const reason = await fork({ chatId, turnId });
            busyRef.current = false;
            setResult(reason);
            if (reason === null) onClose();
          }}
        >
          <GitBranch size={14} />
          <span>
            <strong>Fork in this workspace</strong>
            <small>Fork from this message in the current workspace</small>
          </span>
        </button>
      </div>
      {result && <p role="alert">Couldn&apos;t fork this chat. {result}</p>}
    </ModalDialog>
  );
}

/** Where the fork's own history begins; returns to the parent Chat. */
export function ForkDivider({
  parentChatId,
  forkTurnId,
}: {
  parentChatId: string;
  forkTurnId?: string;
}) {
  const open = useWorkbenchStore((state) =>
    state.tabs.some((tab) => tab.kind === "chat" && tab.id === parentChatId),
  );
  const activateTab = useWorkbenchStore((state) => state.activateTab);
  return (
    <>
      <LazyModuleBoundary
        label="Inherited history"
        load={loadInheritedHistory}
        render={(History) => (
          <History parentChatId={parentChatId} forkTurnId={forkTurnId} />
        )}
      />
      <div className="chat-fork-divider">
        <GitBranch size={14} />
        {open ? (
          <button
            type="button"
            className="chat-fork-divider__link"
            onClick={() => void activateTab(parentChatId)}
          >
            Continued from chat
          </button>
        ) : (
          <span>Continued from an archived chat</span>
        )}
      </div>
    </>
  );
}

const loadInheritedHistory = () =>
  import("./InheritedChatHistory").then(({ InheritedChatHistory }) => ({
    default: InheritedChatHistory,
  }));
