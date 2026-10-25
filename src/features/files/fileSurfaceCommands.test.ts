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
    expect(
      handleFileSurfaceCommandKey(
        event({ key: "g", metaKey: true }),
        mac,
        true,
      ),
    ).toBe(false);
    expect(mac.openGoToLine).not.toHaveBeenCalled();

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
