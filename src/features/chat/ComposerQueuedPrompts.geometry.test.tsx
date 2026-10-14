import { act, cleanup, render } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ComposerQueuedPrompts } from "./ComposerQueuedPrompts";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

it("reserves the actual floating queue height and releases it on compact reflow or removal", () => {
  let queueHeight = 40;
  let resized: ResizeObserverCallback | undefined;
  const disconnect = vi.fn();
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: ResizeObserverCallback) {
        resized = callback;
      }
      observe() {}
      disconnect = disconnect;
    },
  );
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      return this.classList.contains("composer__queued")
        ? new DOMRect(0, 200 - queueHeight - 4, 660, queueHeight)
        : new DOMRect(0, 200, 660, 80);
    },
  );
  const style = document.createElement("style");
  style.textContent = ".composer__queued { position: absolute; }";
  document.head.append(style);
  try {
    const view = render(
      <form className="composer">
        <ComposerQueuedPrompts
          disabled={false}
          prompts={[
            {
              id: "queued",
              text: "Prompt",
              attachedContext: [],
              bindingId: "binding",
              modelId: null,
              status: "blocked",
            },
          ]}
        />
      </form>,
    );
    const composer = view.container.querySelector<HTMLElement>(".composer")!;
    const queue = composer.querySelector<HTMLElement>(".composer__queued")!;
    expect(composer.style.getPropertyValue("--queued-tray-space")).toBe("44px");
    queueHeight = 320;
    act(() => resized?.([], {} as ResizeObserver));
    expect(composer.style.getPropertyValue("--queued-tray-space")).toBe(
      "324px",
    );
    queue.style.position = "static";
    act(() => resized?.([], {} as ResizeObserver));
    expect(composer.style.getPropertyValue("--queued-tray-space")).toBe("0px");
    view.rerender(
      <form className="composer">
        <ComposerQueuedPrompts disabled={false} prompts={[]} />
      </form>,
    );
    expect(composer.style.getPropertyValue("--queued-tray-space")).toBe("");
    expect(disconnect).toHaveBeenCalledOnce();
  } finally {
    style.remove();
  }
});
