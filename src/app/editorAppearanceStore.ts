import { create } from "zustand";

import {
  admitEditorAppearance,
  DEFAULT_EDITOR_APPEARANCE,
  type EditorAppearance,
} from "../features/files/editorAppearance";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";

interface EditorAppearanceStore extends EditorAppearance {
  persistenceClosing: boolean;
  persistenceReady: boolean;
  persistencePhase: "idle" | "loading" | "saving" | "error";
  persistenceErrorKind: "load" | "save" | null;
  persistenceMessage: string | null;
  reset: () => void;
  restore: () => Promise<void>;
  retryPersistence: () => void;
  setAppearance: (appearance: EditorAppearance) => void;
}

let pendingWrite: EditorAppearance | null = null;
let writeDrain: Promise<void> | null = null;
let appearanceGeneration = 0;
let restoreGeneration = 0;
let closeHolds = 0;

const LOAD_FAILURE_MESSAGE = "Editor settings could not be loaded.";
const SAVE_FAILURE_MESSAGE = "Editor settings could not be saved.";

function currentAppearance(state: EditorAppearanceStore): EditorAppearance {
  return admitEditorAppearance({
    fontSize: state.fontSize,
    tabSize: state.tabSize,
    wordWrap: state.wordWrap,
    lineNumbers: state.lineNumbers,
    showWhitespace: state.showWhitespace,
  });
}

async function drainWrites() {
  while (pendingWrite) {
    const appearance = pendingWrite;
    const generation = appearanceGeneration;
    pendingWrite = null;
    try {
      await getWorkbenchPorts().settings.setEditorAppearance(appearance);
      if (!pendingWrite && generation === appearanceGeneration) {
        useEditorAppearanceStore.setState({
          persistencePhase: "idle",
          persistenceErrorKind: null,
          persistenceMessage: null,
        });
      }
    } catch {
      if (!pendingWrite && generation === appearanceGeneration) {
        useEditorAppearanceStore.setState({
          persistencePhase: "error",
          persistenceErrorKind: "save",
          persistenceMessage: SAVE_FAILURE_MESSAGE,
        });
      }
    }
  }
}

function ensureWriteDrain(): Promise<void> {
  if (!writeDrain) {
    writeDrain = drainWrites().finally(() => {
      writeDrain = null;
      if (pendingWrite) void ensureWriteDrain();
    });
  }
  return writeDrain;
}

export async function flushEditorAppearancePersistence(): Promise<void> {
  while (writeDrain || pendingWrite) {
    await ensureWriteDrain();
  }
}

export function holdEditorAppearancePersistenceForClose(): () => void {
  closeHolds += 1;
  useEditorAppearanceStore.setState({ persistenceClosing: true });
  let held = true;
  return () => {
    if (!held) return;
    held = false;
    closeHolds = Math.max(0, closeHolds - 1);
    if (closeHolds === 0) {
      useEditorAppearanceStore.setState({ persistenceClosing: false });
    }
  };
}

function persist(appearance: EditorAppearance) {
  appearanceGeneration += 1;
  pendingWrite = appearance;
  useEditorAppearanceStore.setState({
    persistencePhase: "saving",
    persistenceErrorKind: null,
    persistenceMessage: null,
  });
  void ensureWriteDrain();
}

export const useEditorAppearanceStore = create<EditorAppearanceStore>(
  (set, get) => ({
    ...DEFAULT_EDITOR_APPEARANCE,
    persistenceClosing: false,
    persistenceReady: false,
    persistencePhase: "loading",
    persistenceErrorKind: null,
    persistenceMessage: null,
    reset: () => {
      if (!get().persistenceReady || get().persistenceClosing) return;
      const appearance = admitEditorAppearance(DEFAULT_EDITOR_APPEARANCE);
      set(appearance);
      persist(appearance);
    },
    restore: async () => {
      const appearanceAtStart = appearanceGeneration;
      const request = ++restoreGeneration;
      set({
        persistenceReady: false,
        persistencePhase: "loading",
        persistenceErrorKind: null,
        persistenceMessage: null,
      });
      try {
        const appearance = admitEditorAppearance(
          await getWorkbenchPorts().settings.getEditorAppearance(),
        );
        if (
          request === restoreGeneration &&
          appearanceAtStart === appearanceGeneration
        ) {
          set({
            ...appearance,
            persistenceReady: true,
            persistencePhase: "idle",
            persistenceErrorKind: null,
            persistenceMessage: null,
          });
        }
      } catch {
        if (
          request === restoreGeneration &&
          appearanceAtStart === appearanceGeneration
        ) {
          set({
            persistenceReady: false,
            persistencePhase: "error",
            persistenceErrorKind: "load",
            persistenceMessage: LOAD_FAILURE_MESSAGE,
          });
        }
      }
    },
    retryPersistence: () => {
      if (get().persistenceClosing) return;
      if (get().persistenceErrorKind === "load") {
        void get().restore();
        return;
      }
      persist(currentAppearance(get()));
    },
    setAppearance: (appearance) => {
      if (!get().persistenceReady || get().persistenceClosing) return;
      const admitted = admitEditorAppearance(appearance);
      set(admitted);
      persist(admitted);
    },
  }),
);
