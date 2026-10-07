import { create } from "zustand";

import {
  DEFAULT_UI_APPEARANCE,
  admitUiAppearance,
  type UiAppearance,
} from "../domain/uiAppearance";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { createSettingWriter } from "./settingWriteQueue";

const LOAD_FAILURE = "Appearance settings could not be loaded.";
const SAVE_FAILURE = "Appearance settings could not be saved.";

interface UiAppearanceStore {
  appearance: UiAppearance;
  ready: boolean;
  error: string | null;
  restore: () => Promise<void>;
  flush: () => Promise<void>;
  setAppearance: (appearance: UiAppearance) => void;
}

const writes = createSettingWriter<UiAppearance>(DEFAULT_UI_APPEARANCE);

export const useUiAppearanceStore = create<UiAppearanceStore>((set, get) => ({
  appearance: DEFAULT_UI_APPEARANCE,
  ready: false,
  error: null,
  restore: async () => {
    try {
      const appearance = admitUiAppearance(
        await getWorkbenchPorts().settings.getUiAppearance(),
      );
      writes.persisted(appearance);
      set({ appearance, ready: true, error: null });
    } catch {
      set({ ready: false, error: LOAD_FAILURE });
    }
  },
  flush: () => writes.idle(),
  setAppearance: (appearance) => {
    if (!get().ready) return;
    const admitted = admitUiAppearance(appearance);
    set({ appearance: admitted, error: null });
    writes.write(
      admitted,
      (value) => getWorkbenchPorts().settings.setUiAppearance(value),
      (persisted) => set({ appearance: persisted, error: SAVE_FAILURE }),
    );
  },
}));
