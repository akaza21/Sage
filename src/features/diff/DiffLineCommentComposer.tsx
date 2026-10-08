import { MessageSquarePlus } from "../../ui/leanIcons";
import { useId, useRef, useState } from "react";

import {
  diffLineCommentContext,
  MAX_ATTACHED_CONTEXT_NOTE_BYTES,
  type DiffLineSide,
} from "../../domain/attachedContext";
import type { AgentDiffRef } from "../../domain/agentDiff";
import { utf8ByteLength } from "../../domain/text";

export function DiffLineCommentComposer({
  diffRef,
  side,
  line,
  onAdd,
}: {
  diffRef: AgentDiffRef;
  side: DiffLineSide;
  line: number;
  onAdd: (context: ReturnType<typeof diffLineCommentContext>) => boolean;
}) {
  const [open, setOpen] = useState(false);
  const [note, setNote] = useState("");
  const triggerRef = useRef<HTMLButtonElement>(null);
  const labelId = useId();
  const lineLabel = `${side === "old" ? "old" : "new"} line ${line}`;

  if (!open) {
    return (
      <button
        ref={(node) => {
          if (node) triggerRef.current = node;
        }}
        className="diff-line__comment-trigger"
        type="button"
        aria-label={`Comment on ${lineLabel}`}
        onClick={() => setOpen(true)}
      >
        <MessageSquarePlus size={14} aria-hidden="true" />
      </button>
    );
  }

  const close = () => {
    setNote("");
    setOpen(false);
    requestAnimationFrame(() => triggerRef.current?.focus());
  };

  return (
    <div
      className="diff-comment-composer"
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          close();
        }
      }}
    >
      <label id={labelId} htmlFor={`${labelId}-input`}>
        Comment on {lineLabel}
      </label>
      <input
        id={`${labelId}-input`}
        aria-labelledby={labelId}
        value={note}
        autoFocus
        onChange={(event) => {
          if (
            utf8ByteLength(event.target.value) <=
            MAX_ATTACHED_CONTEXT_NOTE_BYTES
          ) {
            setNote(event.target.value);
          }
        }}
      />
      <div className="diff-comment-composer__actions">
        <button
          className="button button--compact"
          type="button"
          onClick={close}
        >
          Cancel
        </button>
        <button
          className="button button--compact button--primary"
          type="button"
          disabled={!note.trim()}
          onClick={() => {
            try {
              const context = diffLineCommentContext(
                diffRef,
                side,
                line,
                line,
                note,
              );
              if (onAdd(context)) close();
            } catch {
              // A stale/forged ref fails closed and remains unsent.
            }
          }}
        >
          Add to prompt
        </button>
      </div>
    </div>
  );
}
