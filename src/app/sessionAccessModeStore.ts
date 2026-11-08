import { create } from "zustand";
import {
  admitSessionAccessModes,
  type SessionAccessMode,
  type SessionAccessModes,
} from "../domain/sessionAccessMode";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { createSettingWriter } from "./settingWriteQueue";

interface SessionAccessModeStore {
  /** Per-binding defaults for new and existing chats. */
  modes: SessionAccessModes;
  /**
   * Chats' own modes by session (null: the default), newest last. Read and
   * changed only by `sessionAccessActions`, which loads on demand.
   */
  chatModes: Readonly<Record<string, SessionAccessMode | null>>;
  ready: boolean;
  error: string | null;
  /** The last chat access change's failure; it never blocks sending. */
  chatError: string | null;
  restore: () => Promise<void>;
  settle: () => Promise<boolean>;
  setMode: (bindingId: string, mode: SessionAccessMode) => void;
}
const writes = createSettingWriter<SessionAccessModes>({});
let loading: Promise<void> | null = null;
export const useSessionAccessModeStore = create<SessionAccessModeStore>(
  (set, get) => ({
    modes: {},
    chatModes: {},
    ready: false,
    error: null,
    chatError: null,
    restore: async () => {
      const load = (async () => {
        try {
          const modes = admitSessionAccessModes(
            await getWorkbenchPorts().settings.getSessionAccessModes(),
          );
          writes.persisted(modes);
          set({ modes, ready: true, error: null });
        } catch {
          set({ ready: false, error: "Permission mode could not be loaded." });
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
    setMode: (bindingId, mode) => {
      if (!get().ready) return;
      const modes = admitSessionAccessModes({
        ...get().modes,
        [bindingId]: mode,
      });
      set({ modes, error: null });
      writes.write(
        modes,
        (value) => getWorkbenchPorts().settings.setSessionAccessModes(value),
        (persisted) =>
          set({
            modes: persisted,
            error: "Permission mode could not be saved.",
          }),
      );
    },
  }),
);
