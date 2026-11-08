import { create } from "zustand";
import {
  admitSessionAccessModes,
  FULL_ACCESS,
  type SessionAccessMode,
  type SessionAccessModes,
} from "../domain/sessionAccessMode";
import {
  MAX_FULL_ACCESS_SESSIONS,
  sessionFullAccessPort,
} from "../ports/sessionFullAccessPort";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { createSettingWriter } from "./settingWriteQueue";

interface SessionAccessModeStore {
  /** Per-binding defaults for new and existing chats. */
  modes: SessionAccessModes;
  /** Sessions holding a Full access grant, most recent first. */
  fullAccessSessions: readonly string[];
  ready: boolean;
  error: string | null;
  /** The last Full access request's failure; it never blocks sending. */
  fullAccessError: string | null;
  restore: () => Promise<void>;
  settle: () => Promise<boolean>;
  setMode: (bindingId: string, mode: SessionAccessMode) => void;
  /** Opens a Full access confirmation; resolves to its token. */
  prepareFullAccess: (sessionId: string) => Promise<string | null>;
  grantFullAccess: (sessionId: string, confirmToken: string) => Promise<void>;
  revokeFullAccess: (sessionId: string) => Promise<void>;
}
const writes = createSettingWriter<SessionAccessModes>({});
let loading: Promise<void> | null = null;
const admitSessions = (value: unknown) =>
  (Array.isArray(value) ? value : [])
    .filter(
      (id): id is string =>
        typeof id === "string" && id.length > 0 && id.length <= 256,
    )
    .slice(0, MAX_FULL_ACCESS_SESSIONS);
export const useSessionAccessModeStore = create<SessionAccessModeStore>(
  (set, get) => ({
    modes: {},
    fullAccessSessions: [],
    ready: false,
    error: null,
    fullAccessError: null,
    restore: async () => {
      const load = (async () => {
        try {
          const [stored, granted] = await Promise.all([
            getWorkbenchPorts().settings.getSessionAccessModes(),
            sessionFullAccessPort().list(),
          ]);
          const modes = admitSessionAccessModes(stored);
          writes.persisted(modes);
          set({
            modes,
            fullAccessSessions: admitSessions(granted),
            ready: true,
            error: null,
          });
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
      if (!get().ready || mode === FULL_ACCESS) return;
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
    prepareFullAccess: async (sessionId) => {
      try {
        const { confirmToken } =
          await sessionFullAccessPort().prepare(sessionId);
        return confirmToken;
      } catch {
        set({
          fullAccessError: "Full access could not be prepared. Try again.",
        });
        return null;
      }
    },
    grantFullAccess: async (sessionId, confirmToken) => {
      try {
        await sessionFullAccessPort().grant(sessionId, confirmToken);
        set({
          fullAccessError: null,
          fullAccessSessions: [
            sessionId,
            ...get().fullAccessSessions.filter((id) => id !== sessionId),
          ].slice(0, MAX_FULL_ACCESS_SESSIONS),
        });
      } catch {
        set({ fullAccessError: "Full access was not granted. Confirm again." });
      }
    },
    revokeFullAccess: async (sessionId) => {
      if (!get().fullAccessSessions.includes(sessionId)) return;
      try {
        await sessionFullAccessPort().revoke(sessionId);
        set({
          fullAccessError: null,
          fullAccessSessions: get().fullAccessSessions.filter(
            (id) => id !== sessionId,
          ),
        });
      } catch {
        set({
          fullAccessError: "Full access could not be turned off. Try again.",
        });
      }
    },
  }),
);
