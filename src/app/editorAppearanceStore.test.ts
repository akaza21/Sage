import { afterEach, describe, expect, it, vi } from "vitest";

import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { DEFAULT_EDITOR_APPEARANCE } from "../features/files/editorAppearance";
import { useEditorAppearanceStore } from "./editorAppearanceStore";

const initial = useEditorAppearanceStore.getState();

afterEach(() => {
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
    });

    await useEditorAppearanceStore.getState().restore();

    expect(useEditorAppearanceStore.getState()).toMatchObject({
      fontSize: 16,
      tabSize: 8,
      wordWrap: true,
    });
  });

  it("projects status-bar mutations to the same native record", () => {
    const save = vi
      .spyOn(getWorkbenchPorts().settings, "setEditorAppearance")
      .mockResolvedValue(DEFAULT_EDITOR_APPEARANCE);

    useEditorAppearanceStore.getState().cycleFontSize();

    expect(save).toHaveBeenCalledWith({
      fontSize: 14,
      tabSize: 2,
      wordWrap: false,
    });
  });

  it("persists reset defaults as one admitted appearance record", () => {
    const save = vi
      .spyOn(getWorkbenchPorts().settings, "setEditorAppearance")
      .mockResolvedValue(DEFAULT_EDITOR_APPEARANCE);
    useEditorAppearanceStore.setState({
      fontSize: 16,
      tabSize: 8,
      wordWrap: true,
    });

    useEditorAppearanceStore.getState().reset();

    expect(useEditorAppearanceStore.getState()).toMatchObject(
      DEFAULT_EDITOR_APPEARANCE,
    );
    expect(save).toHaveBeenCalledWith(DEFAULT_EDITOR_APPEARANCE);
  });
});
