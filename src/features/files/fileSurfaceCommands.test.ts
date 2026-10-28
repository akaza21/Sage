import { describe, expect, it, vi } from "vitest";

import { handleFileSurfaceCommandKey } from "./fileSurfaceCommands";

function event(overrides: {
  key: string;
  code?: string;
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
  const moveFind = vi.fn<(direction: 1 | -1) => void>();
  return {
    previewing: false,
    findOpen: false,
    goToOpen: false,
    closePreview: vi.fn(),
    closeFind: vi.fn(),
    closeGoToLine: vi.fn(),
    openFind: vi.fn(),
    openReplace: vi.fn(),
    openGoToLine: vi.fn(),
    goToLocalSymbol: vi.fn(),
    save: vi.fn(),
    ...overrides,
    moveFind,
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

  it("opens Go-to Line on Ctrl+G, which is also the macOS chord, unless Preview is open", () => {
    for (const apple of [false, true]) {
      const opened = actions();
      expect(
        handleFileSurfaceCommandKey(
          event({ key: "g", ctrlKey: true }),
          opened,
          apple,
        ),
      ).toBe(true);
      expect(opened.openGoToLine).toHaveBeenCalledOnce();
    }
    // Command+G is Find Next on macOS, so it never opens Go to Line.
    const mac = actions();
    handleFileSurfaceCommandKey(event({ key: "g", metaKey: true }), mac, true);
    expect(mac.openGoToLine).not.toHaveBeenCalled();
    expect(mac.moveFind).toHaveBeenCalledWith(1);

    const preview = actions({ previewing: true });
    handleFileSurfaceCommandKey(
      event({ key: "g", ctrlKey: true }),
      preview,
      false,
    );
    expect(preview.openGoToLine).not.toHaveBeenCalled();
  });

  it("saves and finds with Command on macOS and Control elsewhere, never the other", () => {
    const chords: Array<[boolean, "ctrlKey" | "metaKey", boolean]> = [
      [false, "ctrlKey", true],
      [false, "metaKey", false],
      [true, "metaKey", true],
      [true, "ctrlKey", false],
    ];
    for (const [apple, modifier, expected] of chords) {
      const handlers = actions();
      handleFileSurfaceCommandKey(
        event({ key: "s", [modifier]: true }),
        handlers,
        apple,
      );
      handleFileSurfaceCommandKey(
        event({ key: "f", [modifier]: true }),
        handlers,
        apple,
      );
      expect(handlers.save).toHaveBeenCalledTimes(expected ? 1 : 0);
      expect(handlers.openFind).toHaveBeenCalledTimes(expected ? 1 : 0);
    }
  });

  it("ignores a chord that holds both Control and Command", () => {
    const handlers = actions();
    for (const apple of [false, true]) {
      handleFileSurfaceCommandKey(
        event({ key: "s", ctrlKey: true, metaKey: true }),
        handlers,
        apple,
      );
    }
    expect(handlers.save).not.toHaveBeenCalled();
  });
  it("keeps Escape dismissing with modifiers held", () => {
    const handlers = actions({ findOpen: true });
    expect(
      handleFileSurfaceCommandKey(
        event({ key: "Escape", shiftKey: true }),
        handlers,
        true,
      ),
    ).toBe(true);
    expect(handlers.closeFind).toHaveBeenCalledOnce();
  });

  it("saves on Shift+Control+S but refuses Shift on Find, Go to Line and AltGr chords", () => {
    const handlers = actions();
    handleFileSurfaceCommandKey(
      event({ key: "S", ctrlKey: true, shiftKey: true }),
      handlers,
      false,
    );
    expect(handlers.save).toHaveBeenCalledOnce();
    handleFileSurfaceCommandKey(
      event({ key: "F", ctrlKey: true, shiftKey: true }),
      handlers,
      false,
    );
    handleFileSurfaceCommandKey(
      event({ key: "G", ctrlKey: true, shiftKey: true }),
      handlers,
      false,
    );
    // AltGr on Windows arrives as Control plus Alt.
    for (const key of ["s", "f", "g"]) {
      handleFileSurfaceCommandKey(
        event({ key, ctrlKey: true, altKey: true }),
        handlers,
        false,
      );
    }
    expect(handlers.save).toHaveBeenCalledOnce();
    expect(handlers.openFind).not.toHaveBeenCalled();
    expect(handlers.openGoToLine).not.toHaveBeenCalled();
  });

  it("opens Replace on Control+H, or Command+Option+F on macOS where Command+H hides the app", () => {
    const pc = actions();
    handleFileSurfaceCommandKey(event({ key: "h", ctrlKey: true }), pc, false);
    handleFileSurfaceCommandKey(
      event({ key: "h", ctrlKey: true, altKey: true }),
      pc,
      false,
    );
    expect(pc.openReplace).toHaveBeenCalledOnce();

    const mac = actions();
    handleFileSurfaceCommandKey(event({ key: "h", metaKey: true }), mac, true);
    expect(mac.openReplace).not.toHaveBeenCalled();
    // Option makes the typed character "ƒ", so the physical key decides.
    expect(
      handleFileSurfaceCommandKey(
        event({ key: "ƒ", code: "KeyF", metaKey: true, altKey: true }),
        mac,
        true,
      ),
    ).toBe(true);
    expect(mac.openReplace).toHaveBeenCalledOnce();
    expect(mac.openFind).not.toHaveBeenCalled();

    const preview = actions({ previewing: true });
    handleFileSurfaceCommandKey(
      event({ key: "h", ctrlKey: true }),
      preview,
      false,
    );
    expect(preview.openReplace).not.toHaveBeenCalled();
  });

  it("steps through matches on F3 and Shift+F3, or Command+G and Shift+Command+G on macOS", () => {
    const pc = actions();
    handleFileSurfaceCommandKey(event({ key: "F3" }), pc, false);
    handleFileSurfaceCommandKey(
      event({ key: "F3", shiftKey: true }),
      pc,
      false,
    );
    handleFileSurfaceCommandKey(event({ key: "F3", ctrlKey: true }), pc, false);
    handleFileSurfaceCommandKey(event({ key: "g", ctrlKey: true }), pc, false);
    expect(pc.moveFind.mock.calls).toEqual([[1], [-1]]);

    const mac = actions();
    handleFileSurfaceCommandKey(event({ key: "g", metaKey: true }), mac, true);
    handleFileSurfaceCommandKey(
      event({ key: "G", metaKey: true, shiftKey: true }),
      mac,
      true,
    );
    handleFileSurfaceCommandKey(event({ key: "F3" }), mac, true);
    expect(mac.moveFind.mock.calls).toEqual([[1], [-1]]);
  });

  it("ignores F12 with Command on macOS", () => {
    const handlers = actions();
    handleFileSurfaceCommandKey(
      event({ key: "F12", metaKey: true }),
      handlers,
      true,
    );
    expect(handlers.goToLocalSymbol).not.toHaveBeenCalled();
  });
});
