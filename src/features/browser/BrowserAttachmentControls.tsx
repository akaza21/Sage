import { useEffect, useRef, useState } from "react";
import { Link2, MessageCircleQuestion, Square, Unlink2 } from "lucide-react";
import {
  pinnedAgentChatOwner,
  usePinnedAgentChatOwner,
} from "../../app/agentChatOwner";
import { useWorkbenchStore } from "../../app/store";
import { parseBrowserPageContext } from "../../domain/attachedContext";
import type { BrowserOperationProjection } from "../../domain/browserOperation";
import { clearBrowserAgentPresence } from "../../app/browserAgentActivityStore";
import {
  attachBrowserTab,
  browserAttachmentStatus,
  browserPageContext,
  browserStatus,
  detachBrowserTab,
  stopBrowserTab,
} from "../../ports/nativeBrowserPort";

interface ChatChoice {
  id: string;
  title: string;
}

export function BrowserToolbarPageActions({
  projectId,
  tabId,
  ready,
  chats,
  operation,
  chatTitle,
  attachmentEpoch,
  onStopped,
  onError,
}: {
  projectId: string | null;
  tabId: string;
  ready: boolean;
  chats: readonly ChatChoice[];
  operation: BrowserOperationProjection | null;
  chatTitle: string;
  attachmentEpoch: number;
  onStopped: () => void;
  onError: (message: string) => void;
}) {
  const [stopping, setStopping] = useState(false);
  function stopAgent() {
    if (stopping) return;
    setStopping(true);
    void stopBrowserTab(tabId)
      .then(() => {
        clearBrowserAgentPresence(tabId);
        onStopped();
      })
      .catch((cause) => onError(String(cause).slice(0, 512)))
      .finally(() => setStopping(false));
  }
  if (!ready && !operation) return null;
  return (
    <div className="browser-surface__page-actions">
      <BrowserAttachmentControls
        key={attachmentEpoch}
        projectId={projectId}
        tabId={tabId}
        ready={ready}
        chats={chats}
      />
      {operation && projectId ? (
        <BrowserAgentPresence
          operation={operation}
          chatTitle={chatTitle}
          busy={stopping}
          onDetach={stopAgent}
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
}: {
  projectId: string | null;
  tabId: string;
  ready: boolean;
  chats: readonly ChatChoice[];
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
    } catch {
      setError("Could not detach this tab.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="browser-surface__attachment">
      <BrowserAskPageButton tabId={tabId} ready={ready} onError={setError} />
      <select
        aria-label="Chat for browser tab"
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

export function BrowserAgentPresence({
  operation,
  chatTitle,
  onDetach,
  busy = false,
}: {
  operation: BrowserOperationProjection;
  chatTitle: string;
  onDetach: () => void;
  busy?: boolean;
}) {
  return (
    <div className="browser-surface__agent-presence">
      <span className="browser-surface__agent-presence-label" role="status">
        {operation.phase === "started"
          ? "Agent has a browser action pending"
          : "Agent used this tab"}
        {` · ${chatTitle}`}
      </span>
      <button
        type="button"
        className="browser-surface__agent-stop"
        aria-label="Stop / Take over"
        title="Revoke this chat's browser authority and take over the tab."
        disabled={busy}
        onClick={onDetach}
      >
        <Square size={14} aria-hidden="true" />
      </button>
    </div>
  );
}
