import { CaseSensitive, ChevronDown, ChevronUp, X } from "lucide-react";
import type { RefObject } from "react";

import { MAX_FIND_QUERY_LENGTH } from "./inFileFind";

interface FileFindBarProps {
  inputRef: RefObject<HTMLInputElement | null>;
  query: string;
  status: string;
  caseSensitive: boolean;
  hasMatches: boolean;
  onQueryChange: (query: string) => void;
  onMove: (direction: 1 | -1) => void;
  onToggleCase: () => void;
  onClose: () => void;
}

export function FileFindBar({
  inputRef,
  query,
  status,
  caseSensitive,
  hasMatches,
  onQueryChange,
  onMove,
  onToggleCase,
  onClose,
}: FileFindBarProps) {
  return (
    <div className="editor-find" role="search" aria-label="Find in file">
      <label className="editor-find__query">
        <span className="sr-only">Find text</span>
        <input
          ref={inputRef}
          type="text"
          aria-label="Find text"
          maxLength={MAX_FIND_QUERY_LENGTH}
          value={query}
          onChange={(event) =>
            onQueryChange(event.target.value.slice(0, MAX_FIND_QUERY_LENGTH))
          }
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              onMove(event.shiftKey ? -1 : 1);
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
        aria-label="Previous match"
        disabled={!hasMatches}
        onClick={() => onMove(-1)}
      >
        <ChevronUp size={14} aria-hidden="true" />
      </button>
      <button
        className="icon-button editor-find__button"
        type="button"
        aria-label="Next match"
        disabled={!hasMatches}
        onClick={() => onMove(1)}
      >
        <ChevronDown size={14} aria-hidden="true" />
      </button>
      <button
        className="icon-button editor-find__button"
        type="button"
        aria-label="Match case"
        aria-pressed={caseSensitive}
        onClick={onToggleCase}
      >
        <CaseSensitive size={14} aria-hidden="true" />
      </button>
      <button
        className="icon-button editor-find__button"
        type="button"
        aria-label="Close find"
        onClick={onClose}
      >
        <X size={14} aria-hidden="true" />
      </button>
    </div>
  );
}
