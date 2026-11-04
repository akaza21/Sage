import { useCallback, useEffect, useRef, useState } from "react";

import { usePortalHost } from "../ui/usePortalHost";

import { IconButton } from "../ui/IconButton";
import { useWorkbenchStore } from "../app/store";
import {
  closeUserTerminal,
  createUserTerminal,
  terminalErrorLabel,
  type UserTerminalScope,
  type UserTerminalSession,
} from "../services/userTerminalDesktop";
import { firstChatTabId } from "./workbenchWindowMode";
import { LazyModuleBoundary } from "../ui/LazyModuleBoundary";
import "./terminal-dock.css";
import {
  ChevronDown,
  ChevronUp,
  Plus,
  TerminalSquare,
  X,
} from "../ui/leanIcons";

/** The VT100 grid a shell starts with until the dock measures its screen. */
const INITIAL_COLUMNS = 80;
const INITIAL_ROWS = 24;
const TERMINAL_MAX_PER_CHAT = 4;
const loadTerminalView = () => import("./TerminalDock");

type SessionEntry = {
  session: UserTerminalSession;
  ended: boolean;
  message: string | null;
};

function selectedScope(
  pinnedChatId: string | null,
  preferredChatId?: string,
): UserTerminalScope | null {
  const state = useWorkbenchStore.getState();
  if (!state.project || state.projectTransitioning) return null;
  const chatId =
    [state.activeTabId, preferredChatId, pinnedChatId].find((id) =>
      state.tabs.some((tab) => tab.kind === "chat" && tab.id === id),
    ) ?? firstChatTabId(state.tabs);
  if (!chatId) return null;
  return {
    projectId: state.project.id,
    projectGeneration: String(state.nativeTransitionEpoch),
    chatId,
  };
}

type WorkbenchDockProps = {
  inert?: boolean;
  setOpen: (open: boolean) => void;
  pinnedChatId: string | null;
};

/** Keep the session owner mounted while the visible center adopts its host. */
export default function TerminalDockForWorkbench({
  container,
  inert,
  setOpen,
  pinnedChatId,
}: WorkbenchDockProps & { container: HTMLElement | null }) {
  const { portal } = usePortalHost(container);
  const onClose = useCallback(() => setOpen(false), [setOpen]);
  const [[scope, uiProjectGeneration]] = useState(
    () =>
      [
        selectedScope(pinnedChatId),
        useWorkbenchStore.getState().projectGeneration,
      ] as const,
  );
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
  return portal(
    scope ? (
      <TerminalDockShell scope={scope} onClose={onClose} inert={inert} />
    ) : (
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
    ),
  );
}

export function TerminalDockShell({
  inert,
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
  const [closing, setClosing] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const ownerRef = useRef({
    entries: [] as SessionEntry[],
    creating: false,
    disposed: false,
    lifecycle: 0,
    closing: false,
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
          if (owner.dismissWhenCreateSettles && !owner.entries.length)
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

  const close = async (sessionId: string) => {
    const owner = ownerRef.current;
    if (owner.closing) return;
    const index = owner.entries.findIndex(
      (entry) => entry.session.sessionId === sessionId,
    );
    if (index < 0) return;
    owner.closing = true;
    setClosing(true);
    try {
      await closeUserTerminal(scope, owner.entries[index].session);
      // Closing is serialized; a concurrent creation can only append.
      const next = owner.entries.filter((_, position) => position !== index);
      owner.entries = next;
      setEntries(next);
      setActiveId((current) =>
        current === sessionId
          ? (next[Math.min(index, next.length - 1)]?.session.sessionId ?? null)
          : current,
      );
      if (!next.length) {
        if (owner.creating) owner.dismissWhenCreateSettles = true;
        else onClose();
      }
    } catch (error) {
      setMessage(terminalErrorLabel(error));
    } finally {
      owner.closing = false;
      setClosing(false);
    }
  };

  const updateStatus = (sessionId: string, status: string, ended?: boolean) => {
    const owner = ownerRef.current;
    const next = owner.entries.map((entry) =>
      entry.session.sessionId === sessionId
        ? { ...entry, ended: entry.ended || Boolean(ended), message: status }
        : entry,
    );
    owner.entries = next;
    setEntries(next);
  };

  const active = entries.find((entry) => entry.session.sessionId === activeId);
  const status = message ?? active?.message;
  const collapseLabel = collapsed ? "Expand terminal" : "Collapse terminal";
  const CollapseIcon = collapsed ? ChevronUp : ChevronDown;
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
          {entries.map(({ session }, index) => (
            <div className="terminal-dock__tab" key={session.sessionId}>
              <button
                type="button"
                role="tab"
                className="terminal-dock__tab-label"
                aria-selected={session.sessionId === activeId}
                title={session.shellPath}
                onClick={() => setActiveId(session.sessionId)}
              >
                <TerminalSquare size={14} />
                <span>
                  {session.shellPath.split(/[\\/]/u).at(-1)} {index + 1}
                </span>
              </button>
              <IconButton
                aria-label="Close terminal tab"
                disabled={closing}
                onClick={() => void close(session.sessionId)}
              >
                <X size={14} />
              </IconButton>
            </div>
          ))}
        </div>
        <div className="terminal-dock__actions">
          <IconButton
            aria-label="New terminal"
            disabled={creating || entries.length >= TERMINAL_MAX_PER_CHAT}
            onClick={create}
          >
            <Plus size={16} />
          </IconButton>
          <IconButton
            aria-label={collapseLabel}
            onClick={() => setCollapsed(!collapsed)}
          >
            <CollapseIcon size={16} />
          </IconButton>
        </div>
      </div>
      {!collapsed && status ? (
        <p className="terminal-dock__message" role="status">
          {status}
        </p>
      ) : null}
      {!collapsed && active ? (
        <LazyModuleBoundary
          label="Terminal view"
          load={loadTerminalView}
          fallback={null}
          render={(TerminalView) => (
            <TerminalView
              key={active.session.sessionId}
              scope={scope}
              session={active.session}
              ended={active.ended}
              onStatus={(status, ended) =>
                updateStatus(active.session.sessionId, status, ended)
              }
            />
          )}
        />
      ) : null}
    </section>
  );
}
