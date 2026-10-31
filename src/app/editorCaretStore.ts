import type { Text } from "@codemirror/state";
import { useEffect } from "react";
import { create } from "zustand";

import {
  admitCaretStatus,
  type EditorCaret,
} from "../features/files/editorCaret";

interface EditorCaretStore {
  caret: EditorCaret | null;
  /** False for a read-only File, such as one an Agent shows. */
  editable: boolean;
  publish: (document: Text, offset: number, editable?: boolean) => void;
  clear: () => void;
}

export const useEditorCaretStore = create<EditorCaretStore>((set) => ({
  caret: null,
  editable: true,
  publish: (document, offset, editable = true) =>
    set({ caret: admitCaretStatus(document, offset), editable }),
  clear: () => set({ caret: null }),
}));

/**
 * `active` says whether a caret applies. The editor passes its own document
 * with each offset, so the caret never pairs a new offset with the text of an
 * earlier render.
 */
export function useEditorCaretPublisher(active: boolean, editable = true) {
  useEffect(() => {
    if (!active) {
      useEditorCaretStore.getState().clear();
    }
  }, [active]);

  useEffect(
    () => () => {
      useEditorCaretStore.getState().clear();
    },
    [],
  );

  return (offset: number, document: Text) => {
    if (!active) {
      useEditorCaretStore.getState().clear();
      return;
    }
    useEditorCaretStore.getState().publish(document, offset, editable);
  };
}
