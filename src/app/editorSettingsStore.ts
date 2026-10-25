import { create } from "zustand";

import {
  admitEditorSettings,
  DEFAULT_EDITOR_SETTINGS,
  type EditorSettings,
} from "../domain/editorSettingsCatalogue";
import type { EditorAppearance } from "../features/files/editorAppearance";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { bindEditorSettingsClose } from "./workspaceNotifyLifecycle";

/** The settings the CodeMirror runtime draws with. */
export function editorAppearanceFrom(
  settings: EditorSettings,
): EditorAppearance {
  return {
    fontSize: settings["editor.fontSize"],
    tabSize: settings["editor.tabSize"],
    wordWrap: settings["editor.wordWrap"],
    lineNumbers: settings["editor.lineNumbers"],
    showWhitespace: settings["editor.renderWhitespace"],
  };
}

interface EditorSettingsStore {
  settings: EditorSettings;
  persistenceClosing: boolean;
  persistenceReady: boolean;
  persistencePhase: "idle" | "loading" | "saving" | "error";
  persistenceErrorKind: "load" | "save" | null;
  persistenceMessage: string | null;
  reset: () => void;
  restore: () => Promise<void>;
  retryPersistence: () => void;
  setSettings: (changes: Partial<EditorSettings>) => void;
}

// Changed keys not yet sent. Later changes to a key replace earlier ones, so
// a burst of edits becomes one write.
let pendingWrite: Partial<EditorSettings> | null = null;
// Keys whose last write failed. Writes carry only changed keys, so these ride
// along with the next write until one succeeds.
let failedWrite: Partial<EditorSettings> | null = null;
let writeDrain: Promise<void> | null = null;
let settingsGeneration = 0;
let restoreGeneration = 0;
let closeHolds = 0;

const LOAD_FAILURE_MESSAGE = "Editor settings could not be loaded.";
const SAVE_FAILURE_MESSAGE = "Editor settings could not be saved.";

async function drainWrites() {
  while (pendingWrite) {
    const changes = pendingWrite;
    const generation = settingsGeneration;
    pendingWrite = null;
    try {
      await getWorkbenchPorts().settings.setEditorSettings(changes);
      if (!pendingWrite && generation === settingsGeneration) {
        useEditorSettingsStore.setState({
          persistencePhase: "idle",
          persistenceErrorKind: null,
          persistenceMessage: null,
        });
      }
    } catch {
      // A newer change may have queued while this write was in flight.
      const queued = pendingWrite as Partial<EditorSettings> | null;
      if (queued) {
        pendingWrite = { ...changes, ...queued };
      } else {
        failedWrite = { ...changes, ...failedWrite };
      }
      if (!pendingWrite && generation === settingsGeneration) {
        useEditorSettingsStore.setState({
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

export async function flushEditorSettingsPersistence(): Promise<void> {
  while (writeDrain || pendingWrite) {
    await ensureWriteDrain();
  }
}

export function holdEditorSettingsPersistenceForClose(): () => void {
  closeHolds += 1;
  useEditorSettingsStore.setState({ persistenceClosing: true });
  let held = true;
  return () => {
    if (!held) return;
    held = false;
    closeHolds = Math.max(0, closeHolds - 1);
    if (closeHolds === 0) {
      useEditorSettingsStore.setState({ persistenceClosing: false });
    }
  };
}

function persist(changes: Partial<EditorSettings>) {
  settingsGeneration += 1;
  pendingWrite = { ...failedWrite, ...pendingWrite, ...changes };
  failedWrite = null;
  useEditorSettingsStore.setState({
    persistencePhase: "saving",
    persistenceErrorKind: null,
    persistenceMessage: null,
  });
  void ensureWriteDrain();
}

export const useEditorSettingsStore = create<EditorSettingsStore>(
  (set, get) => ({
    settings: DEFAULT_EDITOR_SETTINGS,
    persistenceClosing: false,
    persistenceReady: false,
    persistencePhase: "loading",
    persistenceErrorKind: null,
    persistenceMessage: null,
    reset: () => {
      if (!get().persistenceReady || get().persistenceClosing) return;
      set({ settings: DEFAULT_EDITOR_SETTINGS });
      persist(DEFAULT_EDITOR_SETTINGS);
    },
    restore: async () => {
      const settingsAtStart = settingsGeneration;
      const request = ++restoreGeneration;
      set({
        persistenceReady: false,
        persistencePhase: "loading",
        persistenceErrorKind: null,
        persistenceMessage: null,
      });
      try {
        const settings = admitEditorSettings(
          await getWorkbenchPorts().settings.getEditorSettings(),
        );
        if (
          request === restoreGeneration &&
          settingsAtStart === settingsGeneration
        ) {
          failedWrite = null;
          set({
            settings,
            persistenceReady: true,
            persistencePhase: "idle",
            persistenceErrorKind: null,
            persistenceMessage: null,
          });
        }
      } catch {
        if (
          request === restoreGeneration &&
          settingsAtStart === settingsGeneration
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
      persist(get().settings);
    },
    setSettings: (changes) => {
      if (!get().persistenceReady || get().persistenceClosing) return;
      const current = get().settings;
      const settings = admitEditorSettings({ ...current, ...changes });
      const changed = Object.keys(changes).filter(
        (key) =>
          settings[key as keyof EditorSettings] !==
          current[key as keyof EditorSettings],
      );
      if (changed.length === 0) return;
      set({ settings });
      persist(
        Object.fromEntries(
          changed.map((key) => [key, settings[key as keyof EditorSettings]]),
        ),
      );
    },
  }),
);

bindEditorSettingsClose({
  hold: holdEditorSettingsPersistenceForClose,
  flush: flushEditorSettingsPersistence,
});
