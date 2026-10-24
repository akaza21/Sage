import { useEffect, useId, useRef, useState } from "react";

import {
  commitIdentityChatResult,
  createChatTransitionContext,
} from "../../app/chatTransitions";
import { currentProjectLayout } from "../../app/projectProjection";
import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage, ProjectGeneration } from "../../domain/acp";
import { getGoalChatPage } from "../../services/acpGoalDesktop";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import type { ProjectTransitionResult } from "../../domain/workbench";
import { invokeNative, isTauriRuntime } from "../../ports/nativeInvoke";
import type { ForkChatRequest } from "../../ports/workbenchPorts";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { Ellipsis, GitBranch } from "../../ui/leanIcons";
import { MenuPopover } from "../../ui/MenuPopover";
import { ModalDialog } from "../../ui/ModalDialog";
import { isSideChatId, useSideChatState } from "../../workbench/sideChatState";
import "./fork-chat.css";

/**
 * Lazy fork chunk: the answer "…" menu, the "Fork chat from here" dialog and
 * the "Continued from chat" link. Rust owns the fork; this only asks for it
 * and adopts the authoritative Project snapshot it returns.
 */

export interface ForkTarget {
  chatId: string;
  turnId: string;
}

function invokeFork(
  request: ForkChatRequest,
): Promise<ProjectTransitionResult> {
  if (isTauriRuntime())
    return invokeNative<ProjectTransitionResult>("fork_chat", { ...request });
  const fork = getWorkbenchPorts().chat.forkChat;
  return fork
    ? fork(request)
    : Promise.reject(new Error("Forking needs the desktop app."));
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
  set({ projectTransitioning: true, error: null });
  const committed = await commitIdentityChatResult({
    set,
    get,
    applyProjection: context.applyProjection,
    loadAuthoritativeProjection: context.loadAuthoritativeProjection,
    failedMessage: "The fork did not commit.",
    invoke: () =>
      invokeFork({
        currentLayout: layout,
        chatId,
        turnId,
        expectedTransitionEpoch: get().nativeTransitionEpoch,
      }),
  });
  if (committed) {
    // Rust activates the fork; it replaces the parent's pane, unpaired.
    const fork = get().activeTabId;
    if (fork && fork !== chatId)
      useSideChatState.setState({ soloChatId: fork });
    return null;
  }
  // The dialog reports the failure inline instead of the global banner.
  const reason = get().error;
  set({ error: null });
  return reason || "Try again.";
}

/**
 * The Turn a fork may be taken through: Rust forks only the latest completed
 * Turn of the live head session whose capabilities admit a native fork, and
 * never a temporary side chat.
 */
function forkTarget(page: AcpChatPage): ForkTarget | null {
  const turn = page.turns.at(-1);
  const forkable =
    turn?.state === "completed" &&
    !isSideChatId(page.chatId) &&
    page.sessions.some(
      (session) =>
        session.id === turn.sessionId &&
        session.id === page.currentSessionId &&
        session.residency === "hot" &&
        session.capabilities.nativeSessionFork,
    );
  return forkable ? { chatId: page.chatId, turnId: turn.id } : null;
}

/** Codex's answer "…" for a forkable answer, otherwise nothing. */
export function AnswerMoreActions({ page }: { page: AcpChatPage }) {
  const target = forkTarget(page);
  return target ? <AnswerMoreMenu {...target} /> : null;
}

/** One item, "Branch in new chat". */
function AnswerMoreMenu(target: ForkTarget) {
  const trigger = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  const [open, setOpen] = useState(false);
  const [forking, setForking] = useState(false);
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
        onClick={() => setOpen((current) => !current)}
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
          className="answer-more-menu__item"
          onClick={() => {
            setOpen(false);
            setForking(true);
          }}
        >
          <GitBranch size={14} aria-hidden="true" />
          <span>Branch in new chat</span>
        </button>
      </MenuPopover>
      {forking ? (
        <ForkChatDialog
          {...target}
          onClose={() => {
            setForking(false);
            trigger.current?.focus();
          }}
        />
      ) : null}
    </>
  );
}

/**
 * "Fork in a new worktree" is deliberately absent: Sage Chats have no
 * worktree support, and worktrees are never the project model.
 */
export function ForkChatDialog({
  chatId,
  turnId,
  onClose,
  fork = forkChat,
}: ForkTarget & {
  onClose: () => void;
  fork?: (target: ForkTarget) => Promise<string | null>;
}) {
  const title = useId();
  const option = useRef<HTMLButtonElement>(null);
  const busyRef = useRef(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dismiss = () => {
    if (!busyRef.current) onClose();
  };
  useEffect(() => option.current?.focus(), []);
  useEffect(() => {
    const dialog = option.current?.closest("dialog");
    // A click outside the dialog's own box landed on its backdrop.
    const backdrop = (event: MouseEvent) => {
      const box = dialog?.getBoundingClientRect();
      if (
        event.target === dialog &&
        box &&
        !busyRef.current &&
        (event.clientX < box.left ||
          event.clientX > box.right ||
          event.clientY < box.top ||
          event.clientY > box.bottom)
      )
        onClose();
    };
    dialog?.addEventListener("click", backdrop);
    return () => dialog?.removeEventListener("click", backdrop);
  }, [onClose]);
  const settle = (next: boolean) => {
    busyRef.current = next;
    setBusy(next);
  };
  return (
    <ModalDialog
      className="fork-chat-dialog"
      labelledBy={title}
      onDismiss={dismiss}
    >
      <h2 id={title}>Fork chat from here</h2>
      <div
        role="menu"
        aria-labelledby={title}
        aria-busy={busy || undefined}
        className="fork-chat-dialog__options"
      >
        <button
          ref={option}
          type="button"
          role="menuitem"
          disabled={busy}
          onClick={async () => {
            settle(true);
            setError(null);
            const reason = await fork({ chatId, turnId });
            settle(false);
            if (reason === null) onClose();
            else setError(reason);
          }}
        >
          <GitBranch size={14} aria-hidden="true" />
          <span>
            <strong>Fork in this workspace</strong>
            <small>Fork from this message in the current workspace</small>
          </span>
        </button>
      </div>
      {error ? (
        <p role="alert" className="fork-chat-dialog__error">
          Couldn&apos;t fork this chat. {error}
        </p>
      ) : null}
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
  const inherited = useInheritedHistory(parentChatId, forkTurnId);
  return (
    <>
      {inherited ? (
        // The parent's history through the fork point, read-only, as Codex
        // shows it above the divider (s2/f0066). Turns only: the fork's own
        // transcript keeps the only navigator, find and virtual list.
        <div
          className="chat-fork-inherited"
          aria-label="History from the parent chat"
          role="group"
          inert
        >
          {inherited.earlier ? (
            <p className="acp-history-notice">
              Earlier history isn&apos;t shown.
            </p>
          ) : null}
          <LazyModuleBoundary
            label="Inherited history"
            load={loadInheritedTurns}
            render={(InheritedTurns) => (
              <InheritedTurns page={inherited.page} />
            )}
          />
        </div>
      ) : null}
      <div className="chat-fork-divider">
        <GitBranch size={14} aria-hidden="true" />
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

/** At most this many inherited parent Turns render above the divider. */
const MAX_INHERITED_TURNS = 20;

const loadInheritedTurns = () =>
  import("./acp/AcpChatPresentation").then(({ InheritedTurns }) => ({
    default: InheritedTurns,
  }));

type Inherited = {
  parentChatId: string;
  forkTurnId: string;
  page: AcpChatPage;
  /** Parent Turns before the shown ones exist but are not rendered. */
  earlier: boolean;
};

/**
 * The parent's latest page cut at the fork Turn: one bounded page read, at
 * most MAX_INHERITED_TURNS Turns. Earlier parent history (an older page, or
 * Turns past the cap) is announced, not loaded. A fork point older than the
 * parent's latest page shows nothing. A re-fetch after a Project transition
 * keeps the last render until its answer lands, so it never flashes.
 */
function useInheritedHistory(parentChatId: string, forkTurnId?: string) {
  const projectId = useWorkbenchStore((state) => state.project?.id);
  const generation = useWorkbenchStore((state) => state.nativeTransitionEpoch);
  const [inherited, setInherited] = useState<Inherited | null>(null);
  useEffect(() => {
    if (!projectId || !forkTurnId) return;
    let live = true;
    void getGoalChatPage({
      projectId: projectId as AcpChatPage["projectId"],
      projectGeneration: String(generation) as ProjectGeneration,
      chatId: parentChatId as AcpChatPage["chatId"],
      cursor: null,
    })
      .then((page) => {
        if (!live || page.chatId !== parentChatId) return;
        const end = page.turns.findIndex((turn) => turn.id === forkTurnId);
        if (end < 0) return setInherited(null);
        const start = Math.max(0, end + 1 - MAX_INHERITED_TURNS);
        setInherited({
          parentChatId,
          forkTurnId,
          earlier: start > 0 || page.nextCursor !== null,
          page: {
            ...page,
            turns: page.turns.slice(start, end + 1),
            nextCursor: null,
          },
        });
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [projectId, generation, parentChatId, forkTurnId]);
  return inherited?.parentChatId === parentChatId &&
    inherited.forkTurnId === forkTurnId
    ? inherited
    : null;
}
