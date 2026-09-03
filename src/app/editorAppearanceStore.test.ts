import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { DEFAULT_EDITOR_APPEARANCE } from "../features/files/editorAppearance";
import {
  flushEditorAppearancePersistence,
  useEditorAppearanceStore,
} from "./editorAppearanceStore";

const initial = useEditorAppearanceStore.getState();

beforeEach(() => {
  useEditorAppearanceStore.setState(
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
  await flushEditorAppearancePersistence();
  useEditorAppearanceStore.setState(initial, true);
  vi.restoreAllMocks();
});

describe("editor appearance persistence projection", () => {
  it("restores the native appearance before rendering controls", async () => {
    vi.spyOn(
      getWorkbenchPorts().settings,
      "getEditorAppearance",
    ).mockResolvedValue({
      fontSize: 16,
      tabSize: 8,
      wordWrap: true,
      lineNumbers: false,
      showWhitespace: true,
    });

    await useEditorAppearanceStore.getState().restore();

    expect(useEditorAppearanceStore.getState()).toMatchObject({
      fontSize: 16,
      tabSize: 8,
      wordWrap: true,
      lineNumbers: false,
      showWhitespace: true,
    });
  });

  it("projects admitted mutations to the same five-setting native record", async () => {
    const save = vi
      .spyOn(getWorkbenchPorts().settings, "setEditorAppearance")
      .mockResolvedValue(DEFAULT_EDITOR_APPEARANCE);

    useEditorAppearanceStore.getState().setAppearance({
      ...DEFAULT_EDITOR_APPEARANCE,
      fontSize: 14,
    });

    await vi.waitFor(() =>
      expect(save).toHaveBeenCalledWith({
        fontSize: 14,
        tabSize: 2,
        wordWrap: false,
        lineNumbers: true,
        showWhitespace: false,
      }),
    );
  });

  it("blocks edits until restore succeeds so defaults cannot overwrite durable values", async () => {
    let finishLoad:
      ((appearance: typeof DEFAULT_EDITOR_APPEARANCE) => void) | undefined;
    const load = new Promise<typeof DEFAULT_EDITOR_APPEARANCE>((resolve) => {
      finishLoad = resolve;
    });
    vi.spyOn(
      getWorkbenchPorts().settings,
      "getEditorAppearance",
    ).mockReturnValue(load);
    const save = vi.spyOn(getWorkbenchPorts().settings, "setEditorAppearance");
    const restore = useEditorAppearanceStore.getState().restore();

    useEditorAppearanceStore.getState().setAppearance({
      ...DEFAULT_EDITOR_APPEARANCE,
      fontSize: 16,
    });
    expect(save).not.toHaveBeenCalled();
    expect(useEditorAppearanceStore.getState().persistenceReady).toBe(false);

    finishLoad?.({
      ...DEFAULT_EDITOR_APPEARANCE,
      tabSize: 8,
      showWhitespace: true,
    });
    await restore;
    expect(useEditorAppearanceStore.getState()).toMatchObject({
      persistenceReady: true,
      fontSize: 13,
      tabSize: 8,
      showWhitespace: true,
    });
  });

  it("persists reset defaults as one admitted appearance record", async () => {
    const save = vi
      .spyOn(getWorkbenchPorts().settings, "setEditorAppearance")
      .mockResolvedValue(DEFAULT_EDITOR_APPEARANCE);
    useEditorAppearanceStore.setState({
      fontSize: 16,
      tabSize: 8,
      wordWrap: true,
      lineNumbers: false,
      showWhitespace: true,
    });

    useEditorAppearanceStore.getState().reset();

    expect(useEditorAppearanceStore.getState()).toMatchObject(
      DEFAULT_EDITOR_APPEARANCE,
    );
    await vi.waitFor(() =>
      expect(save).toHaveBeenCalledWith(DEFAULT_EDITOR_APPEARANCE),
    );
  });

  it("shows a failed save and retries the exact latest admitted values", async () => {
    const save = vi
      .spyOn(getWorkbenchPorts().settings, "setEditorAppearance")
      .mockRejectedValueOnce(new Error("private native failure"))
      .mockResolvedValueOnce({
        ...DEFAULT_EDITOR_APPEARANCE,
        lineNumbers: false,
      });

    useEditorAppearanceStore.getState().setAppearance({
      ...DEFAULT_EDITOR_APPEARANCE,
      lineNumbers: false,
    });

    await vi.waitFor(() =>
      expect(useEditorAppearanceStore.getState()).toMatchObject({
        persistencePhase: "error",
        persistenceErrorKind: "save",
      }),
    );
    expect(useEditorAppearanceStore.getState().persistenceMessage).not.toMatch(
      /private native failure/i,
    );

    useEditorAppearanceStore.getState().retryPersistence();

    await vi.waitFor(() =>
      expect(useEditorAppearanceStore.getState().persistencePhase).toBe("idle"),
    );
    expect(save).toHaveBeenNthCalledWith(2, {
      ...DEFAULT_EDITOR_APPEARANCE,
      lineNumbers: false,
    });
  });

  it("coalesces rapid writes and lets a newer success supersede an older failure", async () => {
    let rejectFirst: ((error: Error) => void) | undefined;
    const first = new Promise<never>((_resolve, reject) => {
      rejectFirst = reject;
    });
    const save = vi
      .spyOn(getWorkbenchPorts().settings, "setEditorAppearance")
      .mockImplementationOnce(() => first)
      .mockResolvedValue(DEFAULT_EDITOR_APPEARANCE);
    const firstValue = { ...DEFAULT_EDITOR_APPEARANCE, fontSize: 14 as const };
    const skippedValue = { ...DEFAULT_EDITOR_APPEARANCE, tabSize: 4 as const };
    const latestValue = { ...DEFAULT_EDITOR_APPEARANCE, wordWrap: true };

    useEditorAppearanceStore.getState().setAppearance(firstValue);
    useEditorAppearanceStore.getState().setAppearance(skippedValue);
    useEditorAppearanceStore.getState().setAppearance(latestValue);
    rejectFirst?.(new Error("older write failed"));

    await vi.waitFor(() => expect(save).toHaveBeenCalledTimes(2));
    await vi.waitFor(() =>
      expect(useEditorAppearanceStore.getState().persistencePhase).toBe("idle"),
    );
    expect(save).toHaveBeenNthCalledWith(1, firstValue);
    expect(save).toHaveBeenNthCalledWith(2, latestValue);
    expect(save).not.toHaveBeenCalledWith(skippedValue);
    expect(useEditorAppearanceStore.getState().persistenceMessage).toBeNull();
  });

  it("flushes the latest coalesced write before close can continue", async () => {
    let finishFirst: (() => void) | undefined;
    const first = new Promise<typeof DEFAULT_EDITOR_APPEARANCE>((resolve) => {
      finishFirst = () => resolve(DEFAULT_EDITOR_APPEARANCE);
    });
    const save = vi
      .spyOn(getWorkbenchPorts().settings, "setEditorAppearance")
      .mockReturnValueOnce(first)
      .mockResolvedValue(DEFAULT_EDITOR_APPEARANCE);
    const firstValue = { ...DEFAULT_EDITOR_APPEARANCE, fontSize: 14 as const };
    const latestValue = { ...DEFAULT_EDITOR_APPEARANCE, tabSize: 4 as const };

    useEditorAppearanceStore.getState().setAppearance(firstValue);
    useEditorAppearanceStore.getState().setAppearance(latestValue);
    let flushed = false;
    const flush = flushEditorAppearancePersistence().then(() => {
      flushed = true;
    });
    await Promise.resolve();
    expect(flushed).toBe(false);

    finishFirst?.();
    await flush;
    expect(save).toHaveBeenNthCalledWith(1, firstValue);
    expect(save).toHaveBeenNthCalledWith(2, latestValue);
  });

  it("keeps safe defaults on restore failure and retries loading", async () => {
    const load = vi
      .spyOn(getWorkbenchPorts().settings, "getEditorAppearance")
      .mockRejectedValueOnce(new Error("private load failure"))
      .mockResolvedValueOnce({
        ...DEFAULT_EDITOR_APPEARANCE,
        showWhitespace: true,
      });

    await expect(
      useEditorAppearanceStore.getState().restore(),
    ).resolves.toBeUndefined();
    expect(useEditorAppearanceStore.getState()).toMatchObject({
      ...DEFAULT_EDITOR_APPEARANCE,
      persistencePhase: "error",
      persistenceErrorKind: "load",
    });

    useEditorAppearanceStore.getState().retryPersistence();

    await vi.waitFor(() =>
      expect(useEditorAppearanceStore.getState()).toMatchObject({
        showWhitespace: true,
        persistencePhase: "idle",
        persistenceErrorKind: null,
      }),
    );
    expect(load).toHaveBeenCalledTimes(2);
  });
});
