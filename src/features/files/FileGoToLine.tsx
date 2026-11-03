import { useRef } from "react";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import type { FileGoToLineBarProps } from "./FileGoToLineBar";
import { MAX_GOTO_LINE_INPUT_CHARS } from "./goToLineLimits";

const loadGoToLineBar = () => import("./FileGoToLineBar");

export type FileGoToLineProps = Omit<
  FileGoToLineBarProps,
  "submitRef" | "pendingSubmitRef"
> & {
  onQueryChange: (query: string) => void;
};

function focusInput(input: HTMLInputElement | null) {
  if (!input) return;
  input.focus();
  input.select();
}

/** One input and its pending command survive the lazy bar's load. */
export function FileGoToLine({ onQueryChange, ...props }: FileGoToLineProps) {
  const pendingSubmitRef = useRef(false);
  const submitRef = useRef<() => void>(null);
  return (
    <div className="editor-find" role="search" aria-label="Go to line">
      <form
        className="editor-find__query"
        action={() => {
          const submit = submitRef.current;
          if (submit) submit();
          else pendingSubmitRef.current = true;
        }}
      >
        <input
          ref={focusInput}
          aria-label="Go to line"
          maxLength={MAX_GOTO_LINE_INPUT_CHARS}
          value={props.query}
          onChange={(event) =>
            onQueryChange(
              event.target.value.slice(0, MAX_GOTO_LINE_INPUT_CHARS),
            )
          }
          onKeyDown={(event) => {
            if (event.key !== "Escape") return;
            event.preventDefault();
            event.stopPropagation();
            pendingSubmitRef.current = false;
            props.onClose();
          }}
        />
      </form>
      <LazyModuleBoundary
        load={loadGoToLineBar}
        label="Go to line"
        fallback={null}
        render={(FileGoToLineBar) => (
          <FileGoToLineBar
            {...props}
            pendingSubmitRef={pendingSubmitRef}
            submitRef={submitRef}
          />
        )}
      />
    </div>
  );
}
