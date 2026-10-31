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
});

it("opens Settings on the page the caller names", async () => {
  await useWorkbenchStore.getState().openSettings("browser");
  expect(useWorkbenchStore.getState().activeTabId).toBe("settings:appearance");
  expect(useSettingsPageRequest.getState().page).toBe("browser");
});

it("switches the page when Settings is already the active tab", async () => {
  await useWorkbenchStore.getState().openSettings();
  expect(useSettingsPageRequest.getState().page).toBeNull();
  await useWorkbenchStore.getState().openSettings("browser");
  expect(useSettingsPageRequest.getState().page).toBe("browser");
});

it("asks for no page when Settings cannot open", async () => {
  useWorkbenchStore.setState({
    flushActiveEditor: vi.fn(async () => {
      throw new Error("Snapshot failed");
    }),
  });
  await useWorkbenchStore.getState().openSettings("browser");
  expect(useSettingsPageRequest.getState().page).toBeNull();
});
