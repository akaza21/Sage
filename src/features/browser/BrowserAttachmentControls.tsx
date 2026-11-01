import { useEffect, useRef, useState } from "react";
import {
  Link2,
  MessageCircleQuestion,
  MessageSquare,
  Unlink2,
} from "../../ui/leanIcons";
import {
  pinnedAgentChatOwner,
  usePinnedAgentChatOwner,
} from "../../app/agentChatOwner";
import { useWorkbenchStore } from "../../app/store";
import { parseBrowserPageContext } from "../../domain/attachedContext";
import type { BrowserOperationProjection } from "../../domain/browserOperation";
import { clearBrowserAgentPresence } from "../../app/browserAgentActivityStore";
import { BrowserAgentPresence } from "./BrowserAgentPresence";
import {
  attachBrowserTab,
  browserAgentControl,
  browserAttachmentStatus,
  browserPageContext,
  browserStatus,
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
 */
function useBrowserAgentControl(tabId: string, refreshKey: string) {
  const [control, setControl] = useState<BrowserAgentControl | null>(null);
  const sequence = useRef(0);
  useEffect(() => {
    const current = ++sequence.current;
    void browserAgentControl(tabId, "agentControl")
      .then((next) => {
        if (current === sequence.current) setControl(next ?? null);
      })
      .catch(() => {
        if (current === sequence.current) setControl(null);
      });
  }, [tabId, refreshKey]);
  return {
    control,
    apply(next: BrowserAgentControl | null) {
      sequence.current += 1;
      setControl(next);
    },
  };
}

export function BrowserToolbarPageActions({
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
  projectId: string | null;
  tabId: string;
  ready: boolean;
  pageRevision: string;
  chats: readonly ChatChoice[];
  operation: BrowserOperationProjection | null;
  attachmentEpoch: number;
  onStopped: () => void;
  onError: (message: string) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [attachments, setAttachments] = useState(0);
  const { control, apply } = useBrowserAgentControl(
    tabId,
    `${attachmentEpoch}\0${attachments}\0${pageRevision}\0${operation?.operationId ?? ""}\0${operation?.updatedAt ?? ""}`,
  );
  const agent =
    control && projectId && control.projectId === projectId ? control : null;
  function run<T>(command: () => Promise<T>, after: (result: T) => void) {
    if (busy) return;
    setBusy(true);
    void command()
      .then(after)
      .catch((cause) => onError(String(cause).slice(0, 512)))
      .finally(() => setBusy(false));
  }
  if (!ready && !agent) return null;
  return (
    <div className="browser-surface__page-actions">
      <BrowserAttachmentControls
        key={attachmentEpoch}
        projectId={projectId}
        tabId={tabId}
        ready={ready}
        chats={chats}
        onChanged={() => setAttachments((current) => current + 1)}
      />
      {agent ? (
        <BrowserAgentPresence
          pending={
            operation?.phase === "started" && operation.chatId === agent.chatId
          }
          userControl={agent.userControl}
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

export function BrowserAttachmentControls({
  projectId,
  tabId,
  ready,
  chats,
  onChanged,
}: {
  projectId: string | null;
  tabId: string;
  ready: boolean;
  chats: readonly ChatChoice[];
  onChanged?: () => void;
}) {
  const [chatId, setChatId] = useState("");
  const [attachedChat, setAttachedChat] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const attachmentRevision = useRef(0);
  const chosen = chats.some((chat) => chat.id === chatId)
    ? chatId
    : (chats[0]?.id ?? "");

  useEffect(() => {
    if (!projectId || !chosen) return;
    let active = true;
    const revision = attachmentRevision.current;
    void browserAttachmentStatus(projectId, chosen)
      .then((status) => {
        if (active && revision === attachmentRevision.current)
          setAttachedChat(status?.tabId === tabId ? chosen : null);
      })
      .catch(() => {
        if (active && revision === attachmentRevision.current)
          setAttachedChat(null);
      });
    return () => {
      active = false;
    };
  }, [projectId, chosen, tabId]);

  async function attach() {
    if (!projectId || !chosen || !ready || busy) return;
    setBusy(true);
    setError(null);
    try {
      await attachBrowserTab(projectId, chosen, tabId);
      attachmentRevision.current += 1;
      setAttachedChat(chosen);
      onChanged?.();
    } catch {
      setError("Could not attach this tab to the selected chat.");
    } finally {
      setBusy(false);
    }
  }

  async function detach() {
    if (!projectId || !attachedChat || busy) return;
    setBusy(true);
    setError(null);
    try {
      await detachBrowserTab(projectId, attachedChat);
      attachmentRevision.current += 1;
      setAttachedChat(null);
      onChanged?.();
    } catch {
      setError("Could not detach this tab.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="browser-surface__attachment">
      <BrowserAskPageButton tabId={tabId} ready={ready} onError={setError} />
      <span className="browser-surface__chat-picker">
        <MessageSquare
          className="browser-surface__chat-icon"
          size={16}
          aria-hidden="true"
        />
        <select
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
      {attachedChat === chosen ? (
        <button
          type="button"
          aria-label="Detach tab from chat"
          title="Detach tab from chat"
          onClick={() => void detach()}
          disabled={busy}
        >
          <Unlink2 size={16} aria-hidden="true" />
        </button>
      ) : (
        <button
          type="button"
          aria-label="Attach tab to chat"
          title="Attach tab to chat"
          onClick={() => void attach()}
          disabled={!projectId || !chosen || !ready || busy}
        >
          <Link2 size={16} aria-hidden="true" />
        </button>
      )}
      {error ? <span role="alert">{error}</span> : null}
    </div>
  );
}

function BrowserAskPageButton({
  tabId,
  ready,
  onError,
}: {
  tabId: string;
  ready: boolean;
  onError: (message: string) => void;
}) {
  const focusedChatId = usePinnedAgentChatOwner();
  const [pending, setPending] = useState(false);

  async function attach() {
    const state = useWorkbenchStore.getState();
    const intent = focusedChatId
      ? state.captureChatOwnership(focusedChatId)
      : null;
    if (!intent || !ready || pending || state.activeTabId !== tabId) return;
    setPending(true);
    try {
      const before = await browserStatus(tabId);
      if (useWorkbenchStore.getState().activeTabId !== tabId)
        throw new Error("Browser page or focused Chat changed; try again.");
      if (before.loading || before.error || !before.url)
        throw new Error("Browser page unavailable; try again.");
      const captured = parseBrowserPageContext(
        await browserPageContext(tabId, before.revision),
      );
      if (useWorkbenchStore.getState().activeTabId !== tabId)
        throw new Error("Browser page or focused Chat changed; try again.");
      const current = await browserStatus(tabId);
      if (
        !captured ||
        captured.tabId !== tabId ||
        current.loading ||
        current.error ||
        current.url !== before.url ||
        current.revision !== before.revision ||
        useWorkbenchStore.getState().activeTabId !== tabId ||
        pinnedAgentChatOwner() !== focusedChatId
      )
        throw new Error("Browser page or focused Chat changed; try again.");
      if (!useWorkbenchStore.getState().attachChatContext(intent, captured)) {
        throw new Error("Could not attach the page to the focused Chat.");
      }
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
      disabled={!ready || !focusedChatId || pending}
      title={!focusedChatId ? "Focus a Chat to ask about this page" : undefined}
      onClick={() => void attach()}
    >
      <MessageCircleQuestion size={16} aria-hidden="true" />
    </button>
  );
}
