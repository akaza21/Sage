import { useLayoutEffect, type RefObject } from "react";
import { X } from "../../ui/leanIcons";
import "./browser-find.css";

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
  useLayoutEffect(() => {
    findField.current?.focus();
    findField.current?.select();
  }, [findField]);
  return (
    <form
      className="browser-surface__find"
      aria-label="Find in page"
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        event.preventDefault();
        event.stopPropagation();
        onClose();
      }}
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
