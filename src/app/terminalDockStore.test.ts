import { beforeEach, expect, it } from "vitest";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { useTerminalDockStore } from "../workbench/workbenchLayoutStorage";

beforeEach(async () => {
  await useTerminalDockStore.getState().flush();
  await getWorkbenchPorts().settings.setTerminalDockVisible(false);
  useTerminalDockStore.setState({ visible: false, ready: false, error: null });
});

it("defaults closed and restores both visibility choices through settings", async () => {
  const store = useTerminalDockStore;
  expect(store.getState().visible).toBe(false);
  await store.getState().restore();
  expect(store.getState().visible).toBe(false);
  store.getState().setVisible(true);
  await store.getState().flush();
  expect(await getWorkbenchPorts().settings.getTerminalDockVisible()).toBe(
    true,
  );
  store.setState({ visible: false, ready: false });
  await store.getState().restore();
  expect(store.getState().visible).toBe(true);
  store.getState().setVisible(false);
  await store.getState().flush();
  store.setState({ visible: true, ready: false });
  await store.getState().restore();
  expect(store.getState().visible).toBe(false);
});
