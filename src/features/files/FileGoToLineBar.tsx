import { useEffect, useRef, useState } from "react";
import { X } from "../../ui/leanIcons";
import { contentText, type EditorContent } from "./editorContent";
import { goToLineSelection, MAX_GOTO_LINE_INPUT_CHARS } from "./goToLine";

export interface FileGoToLineBarProps {
  query: string;
  onQueryChange: (query: string) => void;
  content: EditorContent;
  onSubmit: (selection: { start: number; end: number }) => void;
  onClose: () => void;
}

export function FileGoToLineBar({
  query,
  onQueryChange,
  content,
  onSubmit,
  onClose,
}: FileGoToLineBarProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [invalid, setInvalid] = useState(false);
  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [inputRef]);
  return (
    <div className="editor-find" role="search" aria-label="Go to line">
      <label className="editor-find__query">
        <span className="sr-only">Go to line</span>
        <input
          ref={inputRef}
          type="text"
          aria-label="Go to line"
          maxLength={MAX_GOTO_LINE_INPUT_CHARS}
          value={query}
          onChange={(event) => {
            onQueryChange(
              event.target.value.slice(0, MAX_GOTO_LINE_INPUT_CHARS),
            );
            setInvalid(false);
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              const selection = goToLineSelection(contentText(content), query);
              if (selection) onSubmit(selection);
              else setInvalid(true);
            }
          }}
        />
      </label>
      <span className="editor-find__status" role="status">
        {invalid ? "Enter a line" : "Line or line:column"}
      </span>
      <button
        className="icon-button editor-find__button"
        type="button"
        aria-label="Close go to line"
        onClick={onClose}
      >
        <X size={14} aria-hidden="true" />
      </button>
    </div>
  );
}
