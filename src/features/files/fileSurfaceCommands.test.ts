import { describe, expect, it, vi } from "vitest";

import { handleFileSurfaceCommandKey } from "./fileSurfaceCommands";

function event(overrides: {
  key: string;
  altKey?: boolean;
  ctrlKey?: boolean;
  metaKey?: boolean;
  shiftKey?: boolean;
}) {
  return {
    altKey: false,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    preventDefault: vi.fn(),
    ...overrides,
  };
}

function actions(
  overrides: Partial<Parameters<typeof handleFileSurfaceCommandKey>[1]> = {},
) {
  return {
    previewing: false,
    findOpen: false,
    goToOpen: false,
    closePreview: vi.fn(),
    closeFind: vi.fn(),
    closeGoToLine: vi.fn(),
    openFind: vi.fn(),
    openGoToLine: vi.fn(),
    goToLocalSymbol: vi.fn(),
    save: vi.fn(),
    ...overrides,
  };
}

describe("handleFileSurfaceCommandKey", () => {
  it("routes F12 to bounded local-symbol navigation outside Preview", () => {
    const opened = actions();
    expect(handleFileSurfaceCommandKey(event({ key: "F12" }), opened)).toBe(
      true,
    );
    expect(opened.goToLocalSymbol).toHaveBeenCalledOnce();

    const preview = actions({ previewing: true });
    handleFileSurfaceCommandKey(event({ key: "F12" }), preview);
    expect(preview.goToLocalSymbol).not.toHaveBeenCalled();
  });

  it("opens Go-to Line on Ctrl/Cmd+G unless Preview is open", () => {
    const opened = actions();
    expect(
      handleFileSurfaceCommandKey(event({ key: "g", ctrlKey: true }), opened),
    ).toBe(true);
    expect(opened.openGoToLine).toHaveBeenCalledOnce();

    const mac = actions();
    handleFileSurfaceCommandKey(event({ key: "g", metaKey: true }), mac);
    expect(mac.openGoToLine).toHaveBeenCalledOnce();

    const preview = actions({ previewing: true });
    handleFileSurfaceCommandKey(event({ key: "g", ctrlKey: true }), preview);
    expect(preview.openGoToLine).not.toHaveBeenCalled();
  });
});
