import { useEffect, useReducer, useRef, useState } from "react";
import {
  Link2,
  MessageCircleQuestion,
  MessageSquare,
  Unlink2,
} from "../../ui/leanIcons";
import { usePinnedAgentChatOwner } from "../../app/agentChatOwner";
import { useWorkbenchStore } from "../../app/store";
import type { BrowserOperationProjection } from "../../domain/browserOperation";
import { clearBrowserAgentPresence } from "../../app/browserAgentActivityStore";
import { BrowserAgentPresence } from "./BrowserAgentPresence";
import { subscribeBrowserTab } from "./browserTabEvents";
import { isTauriRuntime } from "../../ports/desktopRuntime";
import {
  attachBrowserTab,
  browserAgentControl,
  browserAttachmentStatus,
  detachBrowserTab,
  stopBrowserTab,
  type BrowserAgentControl,
} from "../../ports/nativeBrowserPort";

interface ChatChoice {
  id: string;
  title: string;
}

/**
 * Who controls the tab, read from Rust whenever anything that can change it
 * happens here: mounting (also after a reload), an Agent operation, a page
 * change, and attaching or detaching. Rust answers Take over and Hand back
 * with the state after them, so nothing here is a mirror that can go stale.
 * Rust also says when something outside the tab changed it, such as the
 * developer-mode switch going off.
 */
function useBrowserAgentControl(
  tabId: string,
  refreshKey: string,
  visible: boolean,
) {
  const [control, setControl] = useState<BrowserAgentControl | null>(null);
  const [nativeChange, refresh] = useReducer(
    (current: number) => current + 1,
    0,
  );
  const sequence = useRef(0);
  const registered = useRef(false);
  useEffect(() => {
    if (!visible || !isTauriRuntime()) return;
    const stop = subscribeBrowserTab(
      "browser:agent-control",
      tabId,
      () => {
        // Invalidate pending reads immediately, before React runs the effect.
        sequence.current += 1;
        refresh();
      },
      async () => {
        registered.current = true;
        refresh();
      },
    );
    return () => {
      registered.current = false;
      sequence.current += 1;
      stop();
    };
  }, [tabId, visible]);
  useEffect(() => {
    if (!visible || (isTauriRuntime() && !registered.current)) return;
    const current = ++sequence.current;
    void browserAgentControl(tabId, "agentControl")
      .catch(() => null)
      .then((next) => {
        if (current === sequence.current) setControl(next);
      });
    return () => {
      sequence.current += 1;
    };
  }, [tabId, refreshKey, nativeChange, visible]);
  return {
    control,
    apply(next: BrowserAgentControl | null) {
      sequence.current += 1;
      setControl(next);
    },
  };
}

export function BrowserToolbarPageActions({
  visible = true,
  projectId,
  tabId,
  ready,
  pageRevision,
  chats,
  operation,
  attachmentEpoch,
  onStopped,
  onError,
}: {
  visible?: boolean;
  projectId: string | null;
  tabId: string;
  ready: boolean;
  pageRevision: string;
  chats: readonly ChatChoice[];
  operation: BrowserOperationProjection | null;
  attachmentEpoch: number;
  onStopped: () => void;
  onError: (message: string | null) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [attachments, setAttachments] = useState(0);
  const { control, apply } = useBrowserAgentControl(
    tabId,
    `${attachmentEpoch}\0${attachments}\0${pageRevision}\0${operation?.operationId ?? ""}\0${operation?.updatedAt ?? ""}`,
    visible,
  );
  const agent = control?.projectId === projectId ? control : null;
  async function run<T>(command: () => Promise<T>, after: (result: T) => void) {
    if (busy) return;
    setBusy(true);
    try {
      after(await command());
    } catch (cause) {
      onError(String(cause).slice(0, 512));
    } finally {
      setBusy(false);
    }
  }
  const attachmentControls = useBrowserAttachmentControls({
    visible: visible && (ready || Boolean(agent)),
    projectId,
    tabId,
    ready,
    chats,
    attachmentEpoch,
    onChanged: () => setAttachments((current) => current + 1),
    onError,
  });
  if (!visible || (!ready && !agent)) return null;
  return (
    <div className="browser-surface__page-actions">
      {attachmentControls}
      {agent ? (
        <BrowserAgentPresence
          pending={
            operation?.phase === "started" && operation.chatId === agent.chatId
          }
          userControl={agent.userControl}
          developerMode={agent.developerMode}
          chatTitle={
            chats.find((chat) => chat.id === agent.chatId)?.title ?? "Chat"
          }
          busy={busy}
          onStop={() =>
            run(
              () => stopBrowserTab(tabId),
              () => {
                apply(null);
                clearBrowserAgentPresence(tabId);
                onStopped();
              },
            )
          }
          onTakeOver={() =>
            run(() => browserAgentControl(tabId, "agentTakeOver"), apply)
          }
          onHandBack={() =>
            run(() => browserAgentControl(tabId, "agentHandBack"), apply)
          }
        />
      ) : null}
    </div>
  );
}

function useBrowserAttachmentControls({
  visible = true,
  attachmentEpoch = 0,
  projectId,
  tabId,
  ready,
  chats,
  onChanged,
  onError: setError,
}: {
  visible?: boolean;
  attachmentEpoch?: number;
  projectId: string | null;
  tabId: string;
  ready: boolean;
  chats: readonly ChatChoice[];
  onChanged?: () => void;
  /**
   * Reports a failure, or null to clear one. The surface shows it in its
   * error row, which takes layout space above the page, because the page is
   * a native window that would cover anything drawn over it.
   */
  onError: (message: string | null) => void;
}) {
  const [pagePending, setPagePending] = useState(false);
  const [chatId, setChatId] = useState("");
  const [attachedChat, setAttachedChat] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const attachmentRevision = useRef(0);
  const chosen = chats.some((chat) => chat.id === chatId)
    ? chatId
    : (chats[0]?.id ?? "");

  useEffect(() => {
    if (!visible || !projectId || !chosen) return;
    let active = true;
    const revision = attachmentRevision.current;
    void browserAttachmentStatus(projectId, chosen)
      .catch(() => null)
      .then((status) => {
        if (active && revision === attachmentRevision.current)
          setAttachedChat(status?.tabId === tabId ? chosen : null);
      });
    return () => {
      active = false;
    };
  }, [projectId, chosen, tabId, visible, attachmentEpoch]);

  async function changeAttachment(action: "attach" | "detach") {
    const attaching = action === "attach";
    const target = attaching ? chosen : attachedChat;
    if (!projectId || !target || (attaching && !ready) || busy) return;
    setBusy(true);
    setError(null);
    try {
      if (attaching) await attachBrowserTab(projectId, target, tabId);
      else await detachBrowserTab(projectId, target);
      attachmentRevision.current += 1;
      setAttachedChat(attaching ? target : null);
      onChanged?.();
    } catch {
      setError(
        attaching
          ? "Could not attach this tab to the selected chat."
          : "Could not detach this tab.",
      );
    } finally {
      setBusy(false);
    }
  }

  const attached = attachedChat === chosen;
  const attachmentLabel = attached
    ? "Detach tab from chat"
    : "Attach tab to chat";
  const AttachmentIcon = attached ? Unlink2 : Link2;
  if (!visible) return null;
  return (
    <div className="browser-surface__attachment">
      <BrowserAskPageButton
        tabId={tabId}
        ready={ready}
        onError={setError}
        pending={pagePending}
        setPending={setPagePending}
      />
      <span className="browser-surface__chat-picker">
        <MessageSquare
          className="browser-surface__chat-icon"
          size={16}
          aria-hidden="true"
        />
        <select
          data-overflow-control="chat"
          aria-label="Chat for browser tab"
          title={chats.find((chat) => chat.id === chosen)?.title}
          value={chosen}
          onChange={(event) => setChatId(event.target.value)}
          disabled={!chats.length || busy}
        >
          {chats.map((chat) => (
            <option key={chat.id} value={chat.id}>
              {chat.title}
            </option>
          ))}
        </select>
      </span>
      <button
        type="button"
        aria-label={attachmentLabel}
        data-overflow-control="attachment"
        title={attachmentLabel}
        onClick={() => void changeAttachment(attached ? "detach" : "attach")}
        disabled={busy || (!attached && (!projectId || !chosen || !ready))}
      >
        <AttachmentIcon size={16} aria-hidden="true" />
      </button>
    </div>
  );
}

function BrowserAskPageButton({
  tabId,
  ready,
  onError,
  pending,
  setPending,
}: {
  pending: boolean;
  setPending: (pending: boolean) => void;
  tabId: string;
  ready: boolean;
  onError: (message: string) => void;
}) {
  const focusedChatId = usePinnedAgentChatOwner();

  async function attach() {
    const state = useWorkbenchStore.getState();
    const intent = focusedChatId
      ? state.captureChatOwnership(focusedChatId)
      : null;
    if (!intent || !ready || pending || state.activeTabId !== tabId) return;
    setPending(true);
    try {
      const { attachBrowserPageContext } =
        await import("./attachBrowserPageContext");
      await attachBrowserPageContext(tabId, focusedChatId, intent);
    } catch (error) {
      onError(String(error).slice(0, 512));
    } finally {
      setPending(false);
    }
  }

  return (
    <button
      type="button"
      aria-label={pending ? "Attaching page…" : "Ask about this page"}
      data-overflow-control="ask"
      disabled={!ready || !focusedChatId || pending}
      title={!focusedChatId ? "Focus a chat to ask about this page" : undefined}
      onClick={attach}
    >
      <MessageCircleQuestion size={16} aria-hidden="true" />
    </button>
  );
}

export function BrowserAttachmentControls(
  props: Parameters<typeof useBrowserAttachmentControls>[0],
) {
  return useBrowserAttachmentControls(props);
}
