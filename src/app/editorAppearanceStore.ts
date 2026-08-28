import { create } from "zustand";

import {
  admitEditorAppearance,
  cycleFontSize,
  cycleTabSize,
  DEFAULT_EDITOR_APPEARANCE,
  type EditorAppearance,
} from "../features/files/editorAppearance";

interface EditorAppearanceStore extends EditorAppearance {
  cycleFontSize: () => void;
  cycleTabSize: () => void;
  toggleWordWrap: () => void;
  reset: () => void;
}

export const useEditorAppearanceStore = create<EditorAppearanceStore>(
  (set, get) => ({
    ...DEFAULT_EDITOR_APPEARANCE,
    cycleFontSize: () => set({ fontSize: cycleFontSize(get().fontSize) }),
    cycleTabSize: () => set({ tabSize: cycleTabSize(get().tabSize) }),
    toggleWordWrap: () => set({ wordWrap: !get().wordWrap }),
    reset: () => set(admitEditorAppearance(DEFAULT_EDITOR_APPEARANCE)),
  }),
);
