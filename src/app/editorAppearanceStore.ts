import { create } from "zustand";

import {
  admitEditorAppearance,
  cycleFontSize,
  cycleTabSize,
  DEFAULT_EDITOR_APPEARANCE,
  type EditorAppearance,
} from "../features/files/editorAppearance";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";

interface EditorAppearanceStore extends EditorAppearance {
  cycleFontSize: () => void;
  cycleTabSize: () => void;
  toggleWordWrap: () => void;
  reset: () => void;
  restore: () => Promise<void>;
  setAppearance: (appearance: EditorAppearance) => void;
}

let pendingWrite: EditorAppearance | null = null;
let writing = false;
let appearanceGeneration = 0;

async function drainWrites() {
  if (writing) return;
  writing = true;
  while (pendingWrite) {
    const appearance = pendingWrite;
    pendingWrite = null;
    try {
      await getWorkbenchPorts().settings.setEditorAppearance(appearance);
    } catch {
      // The WebView remains a projection: a later admitted change retries the latest value.
    }
  }
  writing = false;
}

function persist(appearance: EditorAppearance) {
  appearanceGeneration += 1;
  pendingWrite = appearance;
  void drainWrites();
}

export const useEditorAppearanceStore = create<EditorAppearanceStore>(
  (set, get) => ({
    ...DEFAULT_EDITOR_APPEARANCE,
    cycleFontSize: () => {
      const appearance = admitEditorAppearance({
        ...get(),
        fontSize: cycleFontSize(get().fontSize),
      });
      set(appearance);
      persist(appearance);
    },
    cycleTabSize: () => {
      const appearance = admitEditorAppearance({
        ...get(),
        tabSize: cycleTabSize(get().tabSize),
      });
      set(appearance);
      persist(appearance);
    },
    toggleWordWrap: () => {
      const appearance = admitEditorAppearance({
        ...get(),
        wordWrap: !get().wordWrap,
      });
      set(appearance);
      persist(appearance);
    },
    reset: () => {
      const appearance = admitEditorAppearance(DEFAULT_EDITOR_APPEARANCE);
      set(appearance);
      persist(appearance);
    },
    restore: async () => {
      const generation = appearanceGeneration;
      const appearance = admitEditorAppearance(
        await getWorkbenchPorts().settings.getEditorAppearance(),
      );
      if (generation === appearanceGeneration) set(appearance);
    },
    setAppearance: (appearance) => {
      const admitted = admitEditorAppearance(appearance);
      set(admitted);
      persist(admitted);
    },
  }),
);
