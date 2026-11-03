import { StateEffect } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { StrictMode, useRef, useState } from "react";
import { createRoot } from "react-dom/client";

import {
  EditorAdapter,
  type EditorAdapterHandle,
} from "../../../src/features/files/EditorAdapter";
import { FileGoToLine } from "../../../src/features/files/FileGoToLine";

export function mountGoToLineCommand() {
  // No FileSurface ancestor: the command must handle cancellation itself.
  function GoToLineCommand() {
    const editor = useRef<EditorAdapterHandle>(null);
    const [open, setOpen] = useState(false);
    const [query, setQuery] = useState("");
    const close = () => {
      setOpen(false);
      editor.current?.focus();
    };
    return (
      <section
        aria-label="Standalone command"
        onKeyDown={(event) => {
          if (event.ctrlKey && event.key === "g") {
            event.preventDefault();
            setOpen(true);
          }
        }}
      >
        <EditorAdapter
          ref={editor}
          ariaLabel="Command editor"
          path="command.txt"
          content={"one\ntwo\nthree"}
          disabled={false}
          maxContentLength={1024}
          onChange={() => undefined}
        />
        {open && (
          <FileGoToLine
            query={query}
            onQueryChange={setQuery}
            content={"one\ntwo\nthree"}
            onSubmit={({ start, end }) => {
              editor.current?.setSelection(start, end);
              close();
            }}
            onClose={close}
          />
        )}
      </section>
    );
  }

  const host = document.createElement("div");
  document.body.append(host);
  createRoot(host).render(
    <StrictMode>
      <GoToLineCommand />
    </StrictMode>,
  );
}

/** Record real selection transactions, including repeated identical jumps. */
export function observeSelections(element: HTMLElement) {
  const selections: { anchor: number; head: number }[] = [];
  element.dataset.selections = JSON.stringify(selections);
  EditorView.findFromDOM(element)!.dispatch({
    effects: StateEffect.appendConfig.of(
      EditorView.updateListener.of((update) => {
        for (const transaction of update.transactions) {
          if (transaction.selection) {
            const { anchor, head } = transaction.selection.main;
            selections.push({ anchor, head });
            element.dataset.selections = JSON.stringify(selections);
          }
        }
      }),
    ),
  });
}
