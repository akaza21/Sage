import { afterEach, expect, it, vi } from "vitest";

import { DEFAULT_EDITOR_SETTINGS } from "../domain/editorSettingsCatalogue";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import {
  useEditorSettingsStore,
  flushEditorSettingsPersistence,
} from "./editorSettingsStore";
import { installWorkbenchNavigationGuard } from "./installWorkbenchNavigationGuard";
import { useWorkbenchStore } from "./store";

const initial = useWorkbenchStore.getState();
const initialSettings = useEditorSettingsStore.getState();
let dispose: (() => void) | undefined;

afterEach(async () => {
  dispose?.();
  await flushEditorSettingsPersistence();
  useWorkbenchStore.setState(initial, true);
  useEditorSettingsStore.setState(initialSettings, true);
  vi.restoreAllMocks();
});

function navigate() {
  const event = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(event);
  return event.defaultPrevented;
}

it.each(["file", "diff"] as const)(
  "persists a layout-only %s tab change before navigation",
  async (kind) => {
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      projectTransitioning: false,
      tabs: [],
      activeEditor: null,
    });
    let stored: unknown;
    vi.spyOn(
      getWorkbenchPorts().project,
      "persistProjectLayout",
    ).mockImplementation(async (request) => {
      stored = request;
    });
    dispose = installWorkbenchNavigationGuard();
    expect(navigate()).toBe(false);
    const tab = {
      id: "tab-a",
      kind,
      title: "draft.txt",
      position: 0,
      pinned: false,
      dirty: false,
      resource: { rootId: "root-a", relativePath: "draft.txt" },
    };
    useWorkbenchStore.setState({ tabs: [tab], activeTabId: tab.id });
    expect(navigate()).toBe(true);
    await vi.waitFor(() => expect(navigate()).toBe(false));
    expect(stored).toMatchObject({
      projectId: "project-a",
      tabs: [tab],
      activeTabId: tab.id,
    });
    useWorkbenchStore.setState({ tabs: [], activeTabId: null });
    expect(navigate()).toBe(true);
    await vi.waitFor(() => expect(navigate()).toBe(false));
    expect(stored).toMatchObject({ tabs: [], activeTabId: null });
  },
);

it("keeps guarding an unsaved layout after a project transition fails to persist it", async () => {
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    projectTransitioning: false,
    tabs: [],
    activeTabId: null,
    activeEditor: null,
  });
  const save = vi
    .spyOn(getWorkbenchPorts().project, "persistProjectLayout")
    .mockRejectedValue(new Error("disk full"));
  dispose = installWorkbenchNavigationGuard();
  useWorkbenchStore.setState({ selectedRootId: "root-a" });
  await useWorkbenchStore.getState().addProjectRoot();
  expect(useWorkbenchStore.getState().error).toBe("disk full");
  save.mockResolvedValue();
  expect(navigate()).toBe(true);
  await vi.waitFor(() => expect(navigate()).toBe(false));
});

it("guards and drains a second settings change queued behind an in-flight write", async () => {
  useWorkbenchStore.setState({ project: null, activeEditor: null });
  useEditorSettingsStore.setState({
    persistenceReady: true,
    persistencePhase: "idle",
  });
  let finish: (() => void) | undefined;
  const durable = { ...DEFAULT_EDITOR_SETTINGS };
  vi.spyOn(
    getWorkbenchPorts().settings,
    "setEditorSettings",
  ).mockImplementation(
    (changes) =>
      new Promise((resolve) => {
        finish = () => {
          Object.assign(durable, changes);
          resolve(durable);
        };
      }),
  );
  dispose = installWorkbenchNavigationGuard();
  useEditorSettingsStore.getState().setSettings({ "editor.fontSize": 16 });
  const first = finish;
  useEditorSettingsStore.getState().setSettings({ "editor.tabSize": 8 });
  expect(navigate()).toBe(true);
  first?.();
  await vi.waitFor(() => expect(finish).not.toBe(first));
  expect(navigate()).toBe(true);
  finish?.();
  await vi.waitFor(() => expect(navigate()).toBe(false));
  expect(durable).toMatchObject({ "editor.fontSize": 16, "editor.tabSize": 8 });
});
