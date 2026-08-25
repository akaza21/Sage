import { X } from "lucide-react";
import type { RefObject } from "react";

import { MAX_GOTO_LINE_INPUT_CHARS } from "./goToLine";

interface FileGoToLineBarProps {
  inputRef: RefObject<HTMLInputElement | null>;
  query: string;
  status: string;
  onQueryChange: (query: string) => void;
  onSubmit: () => void;
  onClose: () => void;
}

export function FileGoToLineBar({
  inputRef,
  query,
  status,
  onQueryChange,
  onSubmit,
  onClose,
}: FileGoToLineBarProps) {
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
          onChange={(event) =>
            onQueryChange(
              event.target.value.slice(0, MAX_GOTO_LINE_INPUT_CHARS),
            )
          }
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              onSubmit();
            }
          }}
        />
      </label>
      <span className="editor-find__status" role="status">
        {status}
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
