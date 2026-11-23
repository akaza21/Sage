import { create } from "zustand";
import { createSettingWriter } from "./settingWriteQueue";
import {
  getTerminalDockVisible,
  setTerminalDockVisible,
} from "../services/terminalDockSettingsDesktop";
import type { UserTerminalScope } from "../services/userTerminalDesktop";

export type TerminalOpenRequest = {
  scope: UserTerminalScope;
  text: string;
  finish: (error?: Error) => void;
};

interface TerminalDockStore {
  visible: boolean;
  ready: boolean;
  error: string | null;
  pendingOpen: TerminalOpenRequest | null;
  open: (() => void) | null;
  restore: () => Promise<void>;
  flush: () => Promise<void>;
  setVisible: (visible: boolean) => void;
}

const writes = createSettingWriter(false);

export const useTerminalDockStore = create<TerminalDockStore>((set, get) => ({
  visible: false,
  ready: false,
  error: null,
  pendingOpen: null,
  open: null,
  restore: async () => {
    await writes.idle();
    try {
      const visible = (await getTerminalDockVisible()) === true;
      writes.persisted(visible);
      set({ visible, ready: true, error: null });
    } catch {
      set({ ready: false, error: "Terminal visibility could not be loaded." });
    }
  },
  flush: () => writes.idle(),
  setVisible: (visible) => {
    if (!get().ready) return;
    set({ visible, error: null });
    writes.write(visible, setTerminalDockVisible, (persisted) =>
      set({
        visible: persisted,
        error: "Terminal visibility could not be saved.",
      }),
    );
  },
}));
