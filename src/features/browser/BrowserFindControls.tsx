import type { RefObject } from "react";

import { X } from "../../ui/leanIcons";

export function BrowserFindControls({
  findField,
  findText,
  onFindTextChange,
  onFind,
  onClose,
}: {
  findField: RefObject<HTMLInputElement | null>;
  findText: string;
  onFindTextChange: (value: string) => void;
  onFind: (action: "start" | "previous" | "next") => void;
  onClose: () => void;
}) {
  return (
    <form
      className="browser-surface__find"
      onSubmit={(event) => {
        event.preventDefault();
        onFind("start");
      }}
    >
      <input
        ref={findField}
        aria-label="Find text"
        value={findText}
        maxLength={256}
        onChange={(event) => onFindTextChange(event.target.value)}
      />
      <button type="submit" disabled={!findText}>
        Find
      </button>
      <button
        type="button"
        aria-label="Previous match"
        onClick={() => onFind("previous")}
      >
        ↑
      </button>
      <button
        type="button"
        aria-label="Next match"
        onClick={() => onFind("next")}
      >
        ↓
      </button>
      <button type="button" aria-label="Close find" onClick={onClose}>
        <X size={16} aria-hidden="true" />
      </button>
    </form>
  );
}
