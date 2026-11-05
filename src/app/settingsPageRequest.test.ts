import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { useSettingsPageRequest } from "./settingsPageRequest";
import { useWorkbenchStore } from "./store";

const original = useWorkbenchStore.getState();

beforeEach(() => {
  useSettingsPageRequest.setState({ page: null });
  useWorkbenchStore.setState({
    tabs: [],
    activeTabId: null,
    activeEditor: null,
    flushActiveEditor: vi.fn(async () => undefined),
    persistLayout: vi.fn(async () => undefined),
  });
});

afterEach(() => {
  useWorkbenchStore.setState(original, true);
  useAgentSettingsState.getState().close();
  vi.unstubAllGlobals();
});

it.each([
  { compact: true, alreadyActive: false },
  { compact: true, alreadyActive: true },
  { compact: false, alreadyActive: false },
  { compact: false, alreadyActive: true },
])(
  "reveals Settings without changing a desktop rail ($compact, $alreadyActive)",
  async ({ compact, alreadyActive }) => {
    vi.stubGlobal("matchMedia", () => ({ matches: compact }));
    if (alreadyActive) await useWorkbenchStore.getState().openSettings();
    useWorkbenchStore.setState({ sidebarOpen: true });

    await useWorkbenchStore.getState().openSettings();

    expect(useWorkbenchStore.getState().activeTabId).toBe(
      "settings:appearance",
    );
    expect(useWorkbenchStore.getState().sidebarOpen).toBe(!compact);
  },
);

it("leaves the compact drawer open when Settings cannot open", async () => {
  vi.stubGlobal("matchMedia", () => ({ matches: true }));
  useWorkbenchStore.setState({
    sidebarOpen: true,
    flushActiveEditor: async () => {
      throw new Error("Snapshot failed");
    },
  });

  await useWorkbenchStore.getState().openSettings();

  expect(useWorkbenchStore.getState().activeTabId).toBeNull();
  expect(useWorkbenchStore.getState().sidebarOpen).toBe(true);
});

it("opens Settings on the page the caller names", async () => {
  await useWorkbenchStore.getState().openSettings("keyboard");
  expect(useWorkbenchStore.getState().activeTabId).toBe("settings:appearance");
  expect(useSettingsPageRequest.getState().page).toBe("keyboard");
});

it("switches the page when Settings is already the active tab", async () => {
  await useWorkbenchStore.getState().openSettings();
  expect(useSettingsPageRequest.getState().page).toBeNull();
  await useWorkbenchStore.getState().openSettings("keyboard");
  expect(useSettingsPageRequest.getState().page).toBe("keyboard");
});

it("asks for no page when Settings cannot open", async () => {
  useWorkbenchStore.setState({
    flushActiveEditor: vi.fn(async () => {
      throw new Error("Snapshot failed");
    }),
  });
  await useWorkbenchStore.getState().openSettings("keyboard");
  expect(useSettingsPageRequest.getState().page).toBeNull();
});

it("opens Browser in the shared dialog without creating a Settings tab", async () => {
  await useWorkbenchStore.getState().openSettings("browser");
  expect(useAgentSettingsState.getState().page).toBe("Browser");
  expect(useWorkbenchStore.getState().tabs).toEqual([]);
  expect(useWorkbenchStore.getState().activeTabId).toBeNull();
});
