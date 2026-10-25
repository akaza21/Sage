import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { DEFAULT_EDITOR_SETTINGS } from "../domain/editorSettingsCatalogue";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import {
  flushEditorSettingsPersistence,
  holdEditorSettingsPersistenceForClose,
  useEditorSettingsStore,
} from "./editorSettingsStore";

const initial = useEditorSettingsStore.getState();

beforeEach(() => {
  useEditorSettingsStore.setState(
    {
      ...initial,
      persistenceClosing: false,
      persistenceReady: true,
      persistencePhase: "idle",
    },
    true,
  );
});

afterEach(async () => {
  await flushEditorSettingsPersistence();
  useEditorSettingsStore.setState(initial, true);
  vi.restoreAllMocks();
});

const settings = () => useEditorSettingsStore.getState().settings;

describe("editor settings persistence", () => {
  it("restores and admits the stored settings, ignoring unknown keys", async () => {
    vi.spyOn(
      getWorkbenchPorts().settings,
      "getEditorSettings",
    ).mockResolvedValue({
      "editor.fontSize": 15,
      "editor.tabSize": 8,
      "editor.wordWrap": true,
      "editor.lineNumbers": "yes",
      "editor.renderWhitespace": true,
      "editor.fromANewerSage": 1,
    });

    await useEditorSettingsStore.getState().restore();

    expect(settings()).toEqual({
      "editor.fontSize": 14,
      "editor.tabSize": 8,
      "editor.wordWrap": true,
      "editor.lineNumbers": true,
      "editor.renderWhitespace": true,
    });
  });

  it("writes only the keys that changed", async () => {
    const save = vi
      .spyOn(getWorkbenchPorts().settings, "setEditorSettings")
      .mockResolvedValue(DEFAULT_EDITOR_SETTINGS);

    useEditorSettingsStore.getState().setSettings({ "editor.fontSize": 14 });

    expect(settings()["editor.fontSize"]).toBe(14);
    await vi.waitFor(() =>
      expect(save).toHaveBeenCalledWith({ "editor.fontSize": 14 }),
    );
  });

  it("blocks edits until restore succeeds so defaults cannot overwrite durable values", async () => {
    let finishLoad: ((stored: Record<string, unknown>) => void) | undefined;
    const load = new Promise<Record<string, unknown>>((resolve) => {
      finishLoad = resolve;
    });
    vi.spyOn(getWorkbenchPorts().settings, "getEditorSettings").mockReturnValue(
      load,
    );
    const save = vi.spyOn(getWorkbenchPorts().settings, "setEditorSettings");
    const restore = useEditorSettingsStore.getState().restore();

    useEditorSettingsStore.getState().setSettings({ "editor.fontSize": 16 });
    expect(save).not.toHaveBeenCalled();
    expect(useEditorSettingsStore.getState().persistenceReady).toBe(false);

    finishLoad?.({ "editor.tabSize": 8, "editor.renderWhitespace": true });
    await restore;
    expect(useEditorSettingsStore.getState().persistenceReady).toBe(true);
    expect(settings()).toMatchObject({
      "editor.fontSize": 13,
      "editor.tabSize": 8,
      "editor.renderWhitespace": true,
    });
  });

  it("persists reset as every default", async () => {
    const save = vi
      .spyOn(getWorkbenchPorts().settings, "setEditorSettings")
      .mockResolvedValue(DEFAULT_EDITOR_SETTINGS);
    useEditorSettingsStore.setState({
      settings: {
        "editor.fontSize": 16,
        "editor.tabSize": 8,
        "editor.wordWrap": true,
        "editor.lineNumbers": false,
        "editor.renderWhitespace": true,
      },
    });

    useEditorSettingsStore.getState().reset();

    expect(settings()).toEqual(DEFAULT_EDITOR_SETTINGS);
    await vi.waitFor(() =>
      expect(save).toHaveBeenCalledWith(DEFAULT_EDITOR_SETTINGS),
    );
  });

  it("shows a failed save and retries with every current value", async () => {
    const save = vi
      .spyOn(getWorkbenchPorts().settings, "setEditorSettings")
      .mockRejectedValueOnce(new Error("private native failure"))
      .mockResolvedValueOnce(DEFAULT_EDITOR_SETTINGS);

    useEditorSettingsStore.getState().setSettings({
      "editor.lineNumbers": false,
    });

    await vi.waitFor(() =>
      expect(useEditorSettingsStore.getState()).toMatchObject({
        persistencePhase: "error",
        persistenceErrorKind: "save",
      }),
    );
    expect(useEditorSettingsStore.getState().persistenceMessage).not.toMatch(
      /private native failure/i,
    );

    useEditorSettingsStore.getState().retryPersistence();

    await vi.waitFor(() =>
      expect(useEditorSettingsStore.getState().persistencePhase).toBe("idle"),
    );
    expect(save).toHaveBeenNthCalledWith(2, {
      ...DEFAULT_EDITOR_SETTINGS,
      "editor.lineNumbers": false,
    });
  });

  it("merges rapid changes into one write and resends an older failure with it", async () => {
    let rejectFirst: ((error: Error) => void) | undefined;
    const first = new Promise<never>((_resolve, reject) => {
      rejectFirst = reject;
    });
    const save = vi
      .spyOn(getWorkbenchPorts().settings, "setEditorSettings")
      .mockImplementationOnce(() => first)
      .mockResolvedValue(DEFAULT_EDITOR_SETTINGS);

    useEditorSettingsStore.getState().setSettings({ "editor.fontSize": 14 });
    useEditorSettingsStore.getState().setSettings({ "editor.tabSize": 4 });
    useEditorSettingsStore.getState().setSettings({ "editor.tabSize": 8 });
    useEditorSettingsStore.getState().setSettings({ "editor.wordWrap": true });
    rejectFirst?.(new Error("older write failed"));

    await vi.waitFor(() => expect(save).toHaveBeenCalledTimes(2));
    await vi.waitFor(() =>
      expect(useEditorSettingsStore.getState().persistencePhase).toBe("idle"),
    );
    expect(save).toHaveBeenNthCalledWith(1, { "editor.fontSize": 14 });
    expect(save).toHaveBeenNthCalledWith(2, {
      "editor.fontSize": 14,
      "editor.tabSize": 8,
      "editor.wordWrap": true,
    });
    expect(useEditorSettingsStore.getState().persistenceMessage).toBeNull();
  });

  it("keeps a failed key unsaved until a later write carries it", async () => {
    const save = vi
      .spyOn(getWorkbenchPorts().settings, "setEditorSettings")
      .mockRejectedValueOnce(new Error("disk full"))
      .mockResolvedValue(DEFAULT_EDITOR_SETTINGS);

    useEditorSettingsStore.getState().setSettings({ "editor.fontSize": 16 });
    await vi.waitFor(() =>
      expect(useEditorSettingsStore.getState().persistencePhase).toBe("error"),
    );
    useEditorSettingsStore.getState().setSettings({ "editor.wordWrap": true });
    await vi.waitFor(() =>
      expect(useEditorSettingsStore.getState().persistencePhase).toBe("idle"),
    );

    expect(save).toHaveBeenNthCalledWith(2, {
      "editor.fontSize": 16,
      "editor.wordWrap": true,
    });
  });

  it("skips a write when nothing changed", () => {
    const save = vi.spyOn(getWorkbenchPorts().settings, "setEditorSettings");

    useEditorSettingsStore.getState().setSettings({
      "editor.fontSize": DEFAULT_EDITOR_SETTINGS["editor.fontSize"],
    });

    expect(save).not.toHaveBeenCalled();
    expect(useEditorSettingsStore.getState().persistencePhase).toBe("idle");
  });

  it("holds edits during close and flushes the latest write before close continues", async () => {
    let finishFirst: (() => void) | undefined;
    const first = new Promise<Record<string, unknown>>((resolve) => {
      finishFirst = () => resolve(DEFAULT_EDITOR_SETTINGS);
    });
    const save = vi
      .spyOn(getWorkbenchPorts().settings, "setEditorSettings")
      .mockReturnValueOnce(first)
      .mockResolvedValue(DEFAULT_EDITOR_SETTINGS);

    useEditorSettingsStore.getState().setSettings({ "editor.fontSize": 14 });
    useEditorSettingsStore.getState().setSettings({ "editor.tabSize": 4 });
    const release = holdEditorSettingsPersistenceForClose();
    expect(useEditorSettingsStore.getState().persistenceClosing).toBe(true);
    useEditorSettingsStore.getState().setSettings({ "editor.wordWrap": true });
    let flushed = false;
    const flush = flushEditorSettingsPersistence().then(() => {
      flushed = true;
    });
    await Promise.resolve();
    expect(flushed).toBe(false);

    finishFirst?.();
    await flush;
    release();
    expect(useEditorSettingsStore.getState().persistenceClosing).toBe(false);
    expect(save).toHaveBeenNthCalledWith(1, { "editor.fontSize": 14 });
    expect(save).toHaveBeenNthCalledWith(2, { "editor.tabSize": 4 });
    expect(save).toHaveBeenCalledTimes(2);
    expect(settings()["editor.wordWrap"]).toBe(false);
  });

  it("keeps safe defaults on restore failure and retries loading", async () => {
    const load = vi
      .spyOn(getWorkbenchPorts().settings, "getEditorSettings")
      .mockRejectedValueOnce(new Error("private load failure"))
      .mockResolvedValueOnce({ "editor.renderWhitespace": true });

    await expect(
      useEditorSettingsStore.getState().restore(),
    ).resolves.toBeUndefined();
    expect(useEditorSettingsStore.getState()).toMatchObject({
      settings: DEFAULT_EDITOR_SETTINGS,
      persistencePhase: "error",
      persistenceErrorKind: "load",
    });

    useEditorSettingsStore.getState().retryPersistence();

    await vi.waitFor(() =>
      expect(useEditorSettingsStore.getState()).toMatchObject({
        persistencePhase: "idle",
        persistenceErrorKind: null,
      }),
    );
    expect(settings()["editor.renderWhitespace"]).toBe(true);
    expect(load).toHaveBeenCalledTimes(2);
  });
});
