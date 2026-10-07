import { create } from "zustand";

import {
  DEFAULT_SESSION_ACCESS_MODE,
  admitSessionAccessMode,
  type SessionAccessMode,
} from "../domain/sessionAccessMode";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { createSettingWriter } from "./settingWriteQueue";

const LOAD_FAILURE = "Permission mode could not be loaded.";
const SAVE_FAILURE = "Permission mode could not be saved.";

interface SessionAccessModeStore {
  mode: SessionAccessMode;
  ready: boolean;
  error: string | null;
  restore: () => Promise<void>;
  settle: () => Promise<boolean>;
  setMode: (mode: SessionAccessMode) => void;
}

const writes = createSettingWriter<SessionAccessMode>(
  DEFAULT_SESSION_ACCESS_MODE,
);

let loading: Promise<void> | null = null;

export const useSessionAccessModeStore = create<SessionAccessModeStore>(
  (set, get) => ({
    mode: DEFAULT_SESSION_ACCESS_MODE,
    ready: false,
    error: null,
    restore: async () => {
      const load = (async () => {
        try {
          const mode = admitSessionAccessMode(
            await getWorkbenchPorts().settings.getSessionAccessMode(),
          );
          writes.persisted(mode);
          set({ mode, ready: true, error: null });
        } catch {
          set({ ready: false, error: LOAD_FAILURE });
        }
      })();
      loading = load;
      await load;
      if (loading === load) loading = null;
    },
    settle: async () => {
      while (loading) await loading;
      await writes.idle();
      return get().ready && get().error === null;
    },
    setMode: (mode) => {
      if (!get().ready) return;
      const admitted = admitSessionAccessMode(mode);
      set({ mode: admitted, error: null });
      writes.write(
        admitted,
        (value) => getWorkbenchPorts().settings.setSessionAccessMode(value),
        (persisted) => set({ mode: persisted, error: SAVE_FAILURE }),
      );
    },
  }),
);
