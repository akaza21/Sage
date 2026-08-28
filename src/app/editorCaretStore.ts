import { useEffect } from "react";
import { create } from "zustand";

import {
  admitCaretStatus,
  type EditorCaret,
} from "../features/files/editorCaret";

interface EditorCaretStore {
  caret: EditorCaret | null;
  publish: (content: string, offset: number) => void;
  clear: () => void;
}

export const useEditorCaretStore = create<EditorCaretStore>((set) => ({
  caret: null,
  publish: (content, offset) =>
    set({ caret: admitCaretStatus(content, offset) }),
  clear: () => set({ caret: null }),
}));

export function useEditorCaretPublisher(content: string | null) {
  useEffect(() => {
    if (content === null) {
      useEditorCaretStore.getState().clear();
    }
  }, [content]);

  useEffect(
    () => () => {
      useEditorCaretStore.getState().clear();
    },
    [],
  );

  return (offset: number) => {
    if (content === null) {
      useEditorCaretStore.getState().clear();
      return;
    }
    useEditorCaretStore.getState().publish(content, offset);
  };
}
