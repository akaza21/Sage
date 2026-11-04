import type { FormEvent, RefObject } from "react";
import { Check, MessageSquare, X } from "../ui/leanIcons";
import { MAX_CHAT_TITLE_BYTES } from "../domain/chatTitle";

export function ProjectChatRename({
  chatId,
  title,
  draft,
  saving,
  error,
  inputRef,
  onDraft,
  onSubmit,
  onCancel,
}: {
  chatId: string;
  title: string;
  draft: string;
  saving: boolean;
  error: string | null;
  inputRef: RefObject<HTMLInputElement | null>;
  onDraft: (value: string) => void;
  onSubmit: (event: FormEvent) => void;
  onCancel: () => void;
}) {
  const errorId = `chat-rename-error-${chatId}`;
  return (
    <form className="project-chat-edit" onSubmit={onSubmit}>
      <MessageSquare size={14} aria-hidden="true" />
      <input
        ref={inputRef}
        aria-label={`New name for ${title}`}
        aria-invalid={error ? true : undefined}
        aria-describedby={error ? errorId : undefined}
        autoFocus
        disabled={saving}
        maxLength={MAX_CHAT_TITLE_BYTES}
        value={draft}
        onChange={(event) => onDraft(event.currentTarget.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            event.stopPropagation();
            onCancel();
          }
        }}
      />
      <button
        type="submit"
        aria-label="Save chat name"
        title="Save chat name"
        disabled={saving}
      >
        <Check size={14} aria-hidden="true" />
      </button>
      <button
        type="button"
        aria-label="Cancel chat rename"
        title="Cancel chat rename"
        disabled={saving}
        onClick={onCancel}
      >
        <X size={14} aria-hidden="true" />
      </button>
      {error ? (
        <span className="project-chat-edit__error" id={errorId} role="alert">
          {error}
        </span>
      ) : null}
    </form>
  );
}
