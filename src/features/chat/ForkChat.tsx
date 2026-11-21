import {
  useCallback,
  useContext,
  useEffect,
  useId,
  useRef,
  useState,
  type RefObject,
} from "react";

import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import {
  commitIdentityChatResult,
  createChatTransitionContext,
} from "../../app/chatTransitions";
import { currentProjectLayout } from "../../app/projectProjection";
import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage } from "../../domain/acp";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { Copy, Ellipsis, GitBranch } from "../../ui/leanIcons";
import { MenuPopover } from "../../ui/MenuPopover";
import { ModalDialog } from "../../ui/ModalDialog";
import { isSideChatId } from "../../workbench/sideChatState";
import { AnswerActionButton } from "./AnswerActions";
import "./fork-chat.css";
import { inheritComposerModel } from "./inheritedComposerModel";
import {
  ConversationActionContext,
  ConversationRequestContext,
} from "./forkChatRequest";

export function AnswerMoreMenu({
  onCopy,
  onBranch,
  trigger,
}: {
  onCopy: () => void;
  onBranch?: (keyboard: boolean) => void;
  trigger: RefObject<HTMLButtonElement | null>;
}) {
  const menuId = useId();
  const [open, setOpen] = useState(false);
  const keyed = useRef(false);
  const [Icon, label] = onBranch
    ? [GitBranch, "Branch in new chat"]
    : [Copy, "Copy"];
  return (
    <>
      <AnswerActionButton
        ref={trigger}
        label="More actions"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        data-fork-trigger={onBranch ? true : undefined}
        onClick={() => setOpen(!open)}
      >
        <Ellipsis size={14} aria-hidden="true" />
      </AnswerActionButton>
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

// Resolves to null on success or to the reason the fork did not commit.
// eslint-disable-next-line react-refresh/only-export-components -- the chunk's fork action, shared by its parts and tests
export async function forkChat({
  chatId,
  turnId,
}: ForkTarget): Promise<string | null> {
  const { setState: set, getState: get } = useWorkbenchStore;
  const layout = currentProjectLayout(get());
  if (!layout || get().projectTransitioning)
    return "Wait for the current project operation.";
  const context = createChatTransitionContext(set, get);
  const parentConfig =
    acpProjectionStoreFor(chatId).getState().page?.sessionConfig;
  const parentPage = acpProjectionStoreFor(chatId).getState().page;
  const sourceTurn = parentPage?.turns.find((turn) => turn.id === turnId);
  const targets = parentPage?.sessions.find(
    (session) => session.id === sourceTurn?.sessionId,
  )?.capabilities?.rewind?.targets;
  const before = targets !== undefined && targets !== "latestTurn";
  const prompt = before
    ? sourceTurn?.items.find(
        (item) => item.kind === "text" && item.payload.role === "user",
      )
    : undefined;
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
    const fork = get().activeTabId;
    if (fork && fork !== chatId) {
      inheritComposerModel(fork, parentConfig);
      const owner = get().captureChatOwnership(fork);
      if (owner && prompt?.kind === "text")
        get().setChatComposer(owner, prompt.payload.text);
    }
    return null;
  }
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
  forkable?: boolean;
}) {
  const trigger = useRef<HTMLButtonElement>(null);
  const requestFork = useContext(ConversationRequestContext);
  const currentPage = useContext(ConversationActionContext);
  page = currentPage ?? page;
  const turn = page.turns.at(-1);
  const canFork =
    requestFork &&
    forkable &&
    turn?.state === "completed" &&
    turn.sessionId === page.currentSessionId &&
    page.sessions.find((session) => session.id === turn.sessionId)?.capabilities
      ?.rewind?.targets === "latestTurn" &&
    !isSideChatId(page.chatId) &&
    turnSessionForkState(page, turn.sessionId) === "forkable";
  return (
    <AnswerMoreMenu
      onCopy={onCopy}
      trigger={trigger}
      onBranch={
        canFork
          ? (keyboard) =>
              requestFork({ chatId: page.chatId, turnId: turn.id, keyboard })
          : undefined
      }
    />
  );
}

function restoreForkFocus(focusRoot?: RefObject<HTMLDivElement | null>) {
  // A project transition recreates the answer. Look up its live button.
  queueMicrotask(() => {
    focusRoot?.current
      ?.querySelector<HTMLButtonElement>("[data-fork-trigger]")
      ?.focus();
  });
}

/** No worktree fork: Chats lack worktree support; worktrees aren't Projects. */
export function ForkChatDialog({
  chatId,
  turnId,
  onClose,
  keyboard = false,
  fork = forkChat,
  focusRoot,
}: ForkTarget & {
  onClose: () => void;
  keyboard?: boolean;
  fork?: (target: ForkTarget) => Promise<string | null>;
  focusRoot?: RefObject<HTMLDivElement | null>;
}) {
  const title = useId();
  const option = useRef<HTMLButtonElement>(null);
  const busyRef = useRef(false);
  const [result, setResult] = useState<string | null | undefined>(null);
  const busy = result === undefined;
  const [fresh, setFresh] = useState(!keyboard);
  const close = useCallback(() => {
    onClose();
    restoreForkFocus(focusRoot);
  }, [onClose, focusRoot]);
  const dismiss = () => {
    if (!busyRef.current) close();
  };
  const runFork = async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    setResult(undefined);
    let reason: string | null;
    try {
      reason = await fork({ chatId, turnId });
    } catch {
      reason = "Try again.";
    }
    busyRef.current = false;
    setResult(reason);
    if (reason === null) close();
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
        close();
    };
    dialog.addEventListener("click", backdrop);
    return () => dialog.removeEventListener("click", backdrop);
  }, [close]);
  return (
    <ModalDialog
      className="fork-chat-dialog dialog--scrim"
      labelledBy={title}
      onDismiss={dismiss}
    >
      <h2 id={title}>Fork chat from here</h2>
      <p>This chat shares the project folder.</p>
      <div
        role="group"
        aria-labelledby={title}
        aria-busy={busy}
        onKeyDown={() => setFresh(false)}
      >
        <button
          ref={option}
          autoFocus
          className="button--compact"
          type="button"
          data-fresh={fresh || undefined}
          disabled={busy}
          onClick={() => void runFork()}
        >
          <GitBranch size={14} />
          <span>
            <strong>Fork in this workspace</strong>
            <small>Fork from this message in the current workspace</small>
          </span>
        </button>
      </div>
      {result && (
        <p role="alert">
          Couldn&apos;t fork this chat. {result}{" "}
          <button type="button" onClick={() => void runFork()}>
            Retry
          </button>
        </p>
      )}
    </ModalDialog>
  );
}

function turnSessionForkState(page: AcpChatPage, sessionId: string) {
  const session = page.sessions.find(({ id }) => id === sessionId);
  if (!session) return "absent";
  return session.residency === "hot" && session.capabilities?.rewind?.forkAt
    ? "forkable"
    : "present";
}
