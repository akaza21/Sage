import { useCallback, useEffect, useRef, useState } from "react";

import { IconButton } from "../ui/IconButton";
import { useWorkbenchStore } from "../app/store";
import {
  closeUserTerminal,
  createUserTerminal,
  terminalErrorLabel,
  writeUserTerminal,
  type UserTerminalScope,
  type UserTerminalSession,
} from "../services/userTerminalDesktop";
import { LazyModuleBoundary } from "../ui/LazyModuleBoundary";
import {
  ChevronDown,
  ChevronUp,
  Plus,
  TerminalSquare,
  X,
} from "../ui/leanIcons";

const INITIAL_COLUMNS = 80;
const INITIAL_ROWS = 24;
const TERMINAL_MAX_PER_CHAT = 4;
const loadTerminalView = () => import("./TerminalDock");

type SessionEntry = {
  session: UserTerminalSession;
  ended: boolean;
  message: string | null;
};

/** One chat's terminal tabs; the lazy dock entry mounts it for its scope. */
export function TerminalDockPanel({
  inert,
  scope,
  onClose,
  visible,
  open,
}: {
  inert?: boolean;
  scope: UserTerminalScope;
  onClose: () => void;
  visible: boolean;
  open: boolean;
}) {
  const [entries, setEntries] = useState<SessionEntry[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [closing, setClosing] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  if (!open && collapsed) setCollapsed(false);
  const [message, setMessage] = useState<string | null>(null);
  // A pending handoff for this dock's chat and project takes the next new tab;
  // one for another chat or project stays pending until its sender expires it.
  const pending = useWorkbenchStore((state) => state.terminalHandoff);
  const handoff =
    pending?.chatId === scope.chatId && pending.projectId === scope.projectId
      ? pending
      : null;
  const ownerRef = useRef({
    entries: [] as SessionEntry[],
    creating: false,
    lifecycle: 0,
    closing: false,
    dismissWhenCreateSettles: false,
  });

  const create = useCallback(async () => {
    const owner = ownerRef.current;
    // After unmount nothing calls this: the auto-create checks the lifecycle.
    if (owner.creating) return;
    if (owner.entries.length >= TERMINAL_MAX_PER_CHAT)
      return handoff?.run(null);
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
      if (owner.lifecycle !== lifecycle) {
        await closeUserTerminal(scope, session).catch(() => undefined);
        return;
      }
      const next = [...owner.entries, { session, ended: false, message: null }];
      owner.entries = next;
      owner.dismissWhenCreateSettles = false;
      setEntries(next);
      setActiveId(session.sessionId);
      setCollapsed(false);
      // Typed through the keystroke write path, unterminated, so it stays unrun.
      // The sender types nothing once it timed out, and reports a failed write.
      handoff?.run((text) => writeUserTerminal(scope, session, text));
    } catch (error) {
      if (owner.lifecycle === lifecycle) setMessage(terminalErrorLabel(error));
    } finally {
      if (owner.lifecycle === lifecycle) {
        owner.creating = false;
        setCreating(false);
        if (owner.dismissWhenCreateSettles && !owner.entries.length) onClose();
      }
    }
  }, [scope, onClose, handoff]);

  useEffect(() => {
    const owner = ownerRef.current;
    owner.lifecycle += 1;
    return () => {
      owner.lifecycle += 1;
      owner.creating = false;
      for (const entry of owner.entries)
        void closeUserTerminal(scope, entry.session).catch(() => undefined);
      owner.entries = [];
    };
  }, [scope]);

  useEffect(() => {
    const owner = ownerRef.current;
    const lifecycle = owner.lifecycle;
    if (!visible || (entries.length && !handoff)) return;
    // A tab in flight holds `create`; one that lands changes `entries` first.
    queueMicrotask(() => {
      if (owner.lifecycle === lifecycle) void create();
    });
  }, [create, visible, entries.length, handoff]);

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
        ? { ...entry, ended: entry.ended || ended === true, message: status }
        : entry,
    );
    owner.entries = next;
    setEntries(next);
  };

  const status =
    message ??
    entries.find((entry) => entry.session.sessionId === activeId)?.message;
  const CollapseIcon = collapsed ? ChevronUp : ChevronDown;
  return (
    <section
      className={`terminal-dock${collapsed ? " terminal-dock--collapsed" : ""}`}
      aria-label="Terminal"
      hidden={!visible}
      inert={inert}
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
                  {session.shellPath.split(/[\\/]/u).at(-1)}
                  {entries.length > 1 ? ` ${index + 1}` : null}
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
            aria-label={collapsed ? "Expand terminal" : "Collapse terminal"}
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
      {entries.length ? (
        <LazyModuleBoundary
          label="Terminal view"
          load={loadTerminalView}
          fallback={null}
          render={(TerminalView) =>
            entries.map(({ session, ended }) => (
              <TerminalView
                key={session.sessionId}
                scope={scope}
                session={session}
                ended={ended}
                visible={
                  visible && !collapsed && session.sessionId === activeId
                }
                onStatus={(status, ended) =>
                  updateStatus(session.sessionId, status, ended)
                }
              />
            ))
          }
        />
      ) : null}
    </section>
  );
}
