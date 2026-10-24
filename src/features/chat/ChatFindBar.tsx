import { useLayoutEffect, useRef } from "react";
import {
  ArrowDown,
  ArrowUp,
  LoaderCircle,
  Search,
  X,
} from "../../ui/leanIcons";
import { chatFindStatus, MAX_CHAT_FIND_QUERY } from "./chatFindModel";
import "./chat-find.css";

/** Codex "Search chat…" box: one row when empty, a result row once typed. */
export function ChatFindBar({
  query,
  onQuery,
  onClose,
  onStep,
  index,
  count,
  capped,
  pending,
  focusRequest,
  scopeNote,
}: {
  query: string;
  onQuery: (query: string) => void;
  onClose: () => void;
  onStep: (direction: 1 | -1) => void;
  index: number;
  count: number;
  capped: boolean;
  pending: boolean;
  /** Bumped by each Ctrl+F, so a repeat press refocuses and selects. */
  focusRequest: number;
  scopeNote: string | null;
}) {
  const input = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => {
    // 0 is a find resumed after a transcript remount: never steal focus.
    if (focusRequest === 0) return;
    input.current?.focus();
    input.current?.select();
  }, [focusRequest]);
  const Icon = pending ? LoaderCircle : Search;
  const stepDisabled = pending || !count;
  return (
    <section
      className="chat-find"
      role="search"
      aria-label="Find in chat"
      data-pending={pending || undefined}
    >
      <div className="chat-find__input">
        <Icon
          size={14}
          aria-hidden="true"
          className={pending ? "chat-find__spinner" : "chat-find__icon"}
        />
        <input
          ref={input}
          type="search"
          aria-label="Search chat"
          placeholder="Search chat…"
          value={query}
          maxLength={MAX_CHAT_FIND_QUERY}
          spellCheck={false}
          autoComplete="off"
          onChange={(event) =>
            onQuery(event.target.value.slice(0, MAX_CHAT_FIND_QUERY))
          }
        />
        <button
          type="button"
          className="chat-find__close"
          aria-label="Close search"
          onClick={onClose}
        >
          <X size={14} aria-hidden="true" />
        </button>
      </div>
      {query ? (
        <div className="chat-find__results">
          <button
            type="button"
            aria-label="Previous match"
            disabled={stepDisabled}
            onClick={() => onStep(-1)}
          >
            <ArrowUp size={14} aria-hidden="true" />
          </button>
          <button
            type="button"
            aria-label="Next match"
            disabled={stepDisabled}
            onClick={() => onStep(1)}
          >
            <ArrowDown size={14} aria-hidden="true" />
          </button>
          <span role="status" aria-live="polite" aria-atomic="true">
            {chatFindStatus({ pending, index, count, capped })}
          </span>
        </div>
      ) : null}
      {query && scopeNote ? (
        <p className="chat-find__scope">{scopeNote}</p>
      ) : null}
    </section>
  );
}
