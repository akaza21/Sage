import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ComponentType,
} from "react";

import { useWorkbenchStore } from "../app/store";
import {
  closeUserTerminal,
  createUserTerminal,
  TERMINAL_MAX_PER_CHAT,
  terminalErrorLabel,
  type UserTerminalScope,
  type UserTerminalSession,
} from "../services/userTerminalDesktop";
import { firstChatTabId } from "./workbenchWindowMode";
import type { TerminalDockProps } from "./TerminalDock";
import "./terminal-dock.css";

/** The VT100 grid a shell starts with until the dock measures its screen. */
const INITIAL_COLUMNS = 80;
const INITIAL_ROWS = 24;

type SessionEntry = {
  session: UserTerminalSession;
  ended: boolean;
  message: string | null;
};

const loadTerminalView = () =>
  import("./TerminalDock").then(({ TerminalDock }) => ({
    default: TerminalDock,
  }));

function selectedScope(
  pinnedChatId: string | null,
  preferredChatId?: string,
): UserTerminalScope | null {
  const state = useWorkbenchStore.getState();
  if (!state.project || state.projectTransitioning) return null;
  const activeChat = state.tabs.find(
    (tab) => tab.id === state.activeTabId && tab.kind === "chat",
  )?.id;
  const chatId =
    activeChat ??
    (state.tabs.some((tab) => tab.kind === "chat" && tab.id === preferredChatId)
      ? preferredChatId
      : state.tabs.some((tab) => tab.kind === "chat" && tab.id === pinnedChatId)
        ? pinnedChatId
        : firstChatTabId(state.tabs));
  if (!chatId) return null;
  return {
    projectId: state.project.id,
    projectGeneration: String(state.nativeTransitionEpoch),
    chatId,
  };
}

export function TerminalDockForWorkbench({
  inert = false,
  setOpen,
  pinnedChatId,
}: {
  inert?: boolean;
  setOpen: (open: boolean) => void;
  pinnedChatId: string | null;
}) {
  const onClose = useCallback(() => setOpen(false), [setOpen]);
  const [uiProjectGeneration] = useState(
    () => useWorkbenchStore.getState().projectGeneration,
  );
  const [scope] = useState(() => selectedScope(pinnedChatId));
  useEffect(() => {
    if (!scope) return;
    return useWorkbenchStore.subscribe(() => {
      const current = selectedScope(pinnedChatId, scope.chatId);
      if (
        !current ||
        useWorkbenchStore.getState().projectGeneration !==
          uiProjectGeneration ||
        current.projectId !== scope.projectId ||
        current.projectGeneration !== scope.projectGeneration ||
        current.chatId !== scope.chatId
      ) {
        onClose();
      }
    });
  }, [scope, onClose, pinnedChatId, uiProjectGeneration]);
  if (!scope) {
    return (
      <section
        className="terminal-dock"
        aria-label="Terminal"
        inert={inert}
        aria-hidden={inert || undefined}
      >
        <div className="terminal-dock__header">Terminal unavailable</div>
        <p className="terminal-dock__message" role="status">
          Open a chat in an active project to use the terminal.
        </p>
        <button type="button" onClick={onClose}>
          Close terminal
        </button>
      </section>
    );
  }
  return <TerminalDockShell scope={scope} onClose={onClose} inert={inert} />;
}

export function TerminalDockShell({
  inert = false,
  scope,
  onClose,
}: {
  inert?: boolean;
  scope: UserTerminalScope;
  onClose: () => void;
}) {
  const [entries, setEntries] = useState<SessionEntry[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [closingId, setClosingId] = useState<string | null>(null);
  const [collapsed, setCollapsed] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [TerminalView, setTerminalView] =
    useState<ComponentType<TerminalDockProps> | null>(null);
  const ownerRef = useRef({
    entries: [] as SessionEntry[],
    creating: false,
    disposed: false,
    lifecycle: 0,
    closingId: null as string | null,
    dismissWhenCreateSettles: false,
  });

  const create = useCallback(async () => {
    const owner = ownerRef.current;
    if (
      owner.creating ||
      owner.disposed ||
      owner.entries.length >= TERMINAL_MAX_PER_CHAT
    )
      return;
    const lifecycle = owner.lifecycle;
    owner.creating = true;
    setCreating(true);
    setMessage(null);
    try {
      const session = await createUserTerminal(
        scope,
        INITIAL_COLUMNS,
        INITIAL_ROWS,
      );
      if (owner.disposed || owner.lifecycle !== lifecycle) {
        await closeUserTerminal(scope, session).catch(() => undefined);
        return;
      }
      const next = [...owner.entries, { session, ended: false, message: null }];
      owner.entries = next;
      owner.dismissWhenCreateSettles = false;
      setEntries(next);
      setActiveId(session.sessionId);
      setCollapsed(false);
    } catch (error) {
      if (!owner.disposed && owner.lifecycle === lifecycle)
        setMessage(terminalErrorLabel(error));
    } finally {
      if (owner.lifecycle === lifecycle) {
        owner.creating = false;
        if (!owner.disposed) {
          setCreating(false);
          if (owner.dismissWhenCreateSettles && owner.entries.length === 0)
            onClose();
        }
      }
    }
  }, [scope, onClose]);

  useEffect(() => {
    const owner = ownerRef.current;
    owner.disposed = false;
    owner.lifecycle += 1;
    const lifecycle = owner.lifecycle;
    void Promise.resolve().then(() => {
      if (!owner.disposed && owner.lifecycle === lifecycle) void create();
    });
    return () => {
      owner.disposed = true;
      owner.lifecycle += 1;
      owner.creating = false;
      for (const entry of owner.entries)
        void closeUserTerminal(scope, entry.session).catch(() => undefined);
      owner.entries = [];
    };
  }, [create, scope]);

  useEffect(() => {
    if (collapsed || entries.length === 0 || TerminalView) return;
    let cancelled = false;
    void loadTerminalView().then(({ default: View }) => {
      if (!cancelled) setTerminalView(() => View);
    });
    return () => {
      cancelled = true;
    };
  }, [collapsed, entries.length, TerminalView]);

  const close = async (sessionId: string) => {
    const owner = ownerRef.current;
    if (owner.closingId) return;
    const index = owner.entries.findIndex(
      (entry) => entry.session.sessionId === sessionId,
    );
    if (index < 0) return;
    owner.closingId = sessionId;
    setClosingId(sessionId);
    try {
      await closeUserTerminal(scope, owner.entries[index].session);
      const next = owner.entries.filter(
        (entry) => entry.session.sessionId !== sessionId,
      );
      owner.entries = next;
      setEntries(next);
      setActiveId((current) =>
        current === sessionId
          ? (next[Math.min(index, next.length - 1)]?.session.sessionId ?? null)
          : current,
      );
      if (next.length === 0) {
        if (owner.creating) owner.dismissWhenCreateSettles = true;
        else onClose();
      }
    } catch (error) {
      setMessage(terminalErrorLabel(error));
    } finally {
      owner.closingId = null;
      setClosingId(null);
    }
  };

  const updateStatus = (sessionId: string, status: string, ended?: boolean) => {
    const owner = ownerRef.current;
    const next = owner.entries.map((entry) =>
      entry.session.sessionId === sessionId
        ? { ...entry, ended: entry.ended || ended === true, message: status }
        : entry,
    );
    owner.entries = next;
    setEntries(next);
  };

  const active = entries.find((entry) => entry.session.sessionId === activeId);
  return (
    <section
      className={`terminal-dock${collapsed ? " terminal-dock--collapsed" : ""}`}
      aria-label="Terminal"
      inert={inert}
      aria-hidden={inert || undefined}
    >
      <div className="terminal-dock__header">
        <div
          className="terminal-dock__tabs"
          role="tablist"
          aria-label="Terminal tabs"
        >
          {entries.map((entry, index) => (
            <div className="terminal-dock__tab" key={entry.session.sessionId}>
              <button
                type="button"
                role="tab"
                aria-selected={entry.session.sessionId === activeId}
                title={entry.session.shellPath}
                onClick={() => setActiveId(entry.session.sessionId)}
              >
                <span aria-hidden="true">▣</span>{" "}
                {entry.session.shellPath.split(/[\\/]/u).at(-1) ?? "Terminal"}{" "}
                {index + 1}
              </button>
              <button
                type="button"
                aria-label="Close terminal tab"
                disabled={closingId !== null}
                onClick={() => void close(entry.session.sessionId)}
              >
                ×
              </button>
            </div>
          ))}
        </div>
        <div className="terminal-dock__actions">
          <button
            type="button"
            aria-label="New terminal"
            title="New terminal"
            disabled={creating || entries.length >= TERMINAL_MAX_PER_CHAT}
            onClick={() => void create()}
          >
            +
          </button>
          <button
            type="button"
            aria-label={collapsed ? "Expand terminal" : "Collapse terminal"}
            title={collapsed ? "Expand terminal" : "Collapse terminal"}
            onClick={() => setCollapsed(!collapsed)}
          >
            {collapsed ? "⌃" : "⌄"}
          </button>
        </div>
      </div>
      {!collapsed && (message || active?.message) ? (
        <p className="terminal-dock__message" role="status">
          {message ?? active?.message}
        </p>
      ) : null}
      {!collapsed && active && TerminalView ? (
        <TerminalView
          key={active.session.sessionId}
          scope={scope}
          session={active.session}
          ended={active.ended}
          onStatus={(status, ended) =>
            updateStatus(active.session.sessionId, status, ended)
          }
        />
      ) : null}
    </section>
  );
}
