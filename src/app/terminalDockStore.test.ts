import { beforeEach, expect, it, vi } from "vitest";
import {
  getTerminalDockVisible,
  setTerminalDockVisible,
} from "../services/terminalDockSettingsDesktop";

const settings = vi.hoisted(() => ({ visible: false }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (command: string, args?: { visible: boolean }) => {
    if (command === "get_terminal_dock_visible") return settings.visible;
    if (command === "set_terminal_dock_visible") {
      settings.visible = args!.visible;
      return settings.visible;
    }
    throw new Error(`Unexpected settings command: ${command}`);
  },
}));
import { useTerminalDockStore } from "./terminalDockStore";

beforeEach(async () => {
  await useTerminalDockStore.getState().flush();
  await setTerminalDockVisible(false);
  useTerminalDockStore.setState({ visible: false, ready: false, error: null });
});

it("defaults closed and restores both visibility choices through settings", async () => {
  const store = useTerminalDockStore;
  expect(store.getState().visible).toBe(false);
  await store.getState().restore();
  expect(store.getState().visible).toBe(false);
  store.getState().setVisible(true);
  await store.getState().flush();
  expect(await getTerminalDockVisible()).toBe(true);
  store.setState({ visible: false, ready: false });
  await store.getState().restore();
  expect(store.getState().visible).toBe(true);
  store.getState().setVisible(false);
  await store.getState().flush();
  store.setState({ visible: true, ready: false });
  await store.getState().restore();
  expect(store.getState().visible).toBe(false);
});
