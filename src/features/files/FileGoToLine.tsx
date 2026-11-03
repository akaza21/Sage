import { useCallback, useRef } from "react";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import type { FileGoToLineBarProps } from "./FileGoToLineBar";
import { MAX_GOTO_LINE_INPUT_CHARS } from "./goToLineLimits";

const loadGoToLineBar = () =>
  import("./FileGoToLineBar").then((module) => ({
    default: module.FileGoToLineBar,
  }));

export type FileGoToLineProps = Omit<FileGoToLineBarProps, "inputRef">;

/** The input handoff belongs to one open command, including its pending load. */
export function FileGoToLine(props: FileGoToLineProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const focusInput = useCallback((input: HTMLInputElement | null) => {
    if (!input) return;
    // The detached pending input retains the selection for its replacement.
    const previous = inputRef.current;
    inputRef.current = input;
    input.focus({ preventScroll: true });
    if (previous) {
      input.setSelectionRange(
        previous.selectionStart,
        previous.selectionEnd,
        previous.selectionDirection ?? "none",
      );
    } else input.select();
  }, []);
  return (
    <LazyModuleBoundary
      load={loadGoToLineBar}
      label="Go to line"
      fallback={
        <div className="editor-find" role="search" aria-label="Go to line">
          <label className="editor-find__query">
            <input
              ref={focusInput}
              type="text"
              aria-label="Go to line"
              maxLength={MAX_GOTO_LINE_INPUT_CHARS}
              value={props.query}
              onChange={(event) =>
                props.onQueryChange(
                  event.target.value.slice(0, MAX_GOTO_LINE_INPUT_CHARS),
                )
              }
            />
          </label>
        </div>
      }
      render={(FileGoToLineBar) => (
        <FileGoToLineBar {...props} inputRef={focusInput} />
      )}
    />
  );
}
