import type { Text } from "@codemirror/state";
import { useEffect } from "react";
import { create } from "zustand";

import {
  admitCaretStatus,
  type EditorCaret,
} from "../features/files/editorCaret";

interface EditorCaretStore {
  caret: EditorCaret | null;
  publish: (document: Text, offset: number) => void;
  clear: () => void;
}

export const useEditorCaretStore = create<EditorCaretStore>((set) => ({
  caret: null,
  publish: (document, offset) =>
    set({ caret: admitCaretStatus(document, offset) }),
  clear: () => set({ caret: null }),
}));

/**
 * `active` says whether a caret applies. The editor passes its own document
 * with each offset, so the caret never pairs a new offset with the text of an
 * earlier render.
 */
export function useEditorCaretPublisher(active: boolean) {
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
    useEditorCaretStore.getState().publish(document, offset);
  };
}
