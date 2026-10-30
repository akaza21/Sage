import { afterEach, expect, it, vi } from "vitest";
import source from "./annotateDrag.js?raw";

afterEach(() => {
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

function install() {
  const listeners = new Map<string, EventListener>();
  const add = window.addEventListener.bind(window);
  vi.spyOn(window, "addEventListener").mockImplementation(
    (name, listener, options) => {
      listeners.set(name, listener as EventListener);
      add(name, listener, options);
    },
  );
  const report = vi.fn();
  const stop = new Function(
    `return (${source.replace("export default ", "")})`,
  )()(report);
  const mouse = (name: string, x: number, y: number, trusted = true) => {
    const event = {
      isTrusted: trusted,
      button: 0,
      clientX: x,
      clientY: y,
      target: document.body,
      preventDefault: vi.fn(),
      stopImmediatePropagation: vi.fn(),
    };
    listeners.get(name)!(event as unknown as Event);
    return event;
  };
  return { mouse, report, stop };
}

it("tracks a reverse drag locally and reports exactly once on release", () => {
  const { mouse, report, stop } = install();
  mouse("mousedown", 300, 250);
  mouse("mousemove", 100, 80);
  expect(report).not.toHaveBeenCalled();
  mouse("mouseup", 100, 80);
  expect(report).toHaveBeenCalledTimes(1);
  expect(JSON.parse(report.mock.calls[0]![0])).toEqual({
    target: "area",
    x: 100,
    y: 80,
    width: 200,
    height: 170,
  });
  mouse("mouseup", 100, 80);
  expect(report).toHaveBeenCalledTimes(1);
  stop();
});

it("ignores synthetic page events, distinguishes clicks, and cancels on blur", () => {
  const { mouse, report, stop } = install();
  mouse("mousedown", 10, 20, false);
  mouse("mouseup", 200, 200, false);
  expect(report).not.toHaveBeenCalled();
  mouse("mousedown", 10, 20);
  window.dispatchEvent(new Event("blur"));
  mouse("mouseup", 200, 200);
  expect(report).not.toHaveBeenCalled();
  mouse("mousedown", 10, 20);
  mouse("mouseup", 12, 21);
  expect(JSON.parse(report.mock.calls[0]![0])).toEqual({
    target: "element",
    x: 12,
    y: 21,
    width: 0,
    height: 0,
  });
  stop();
});

it("stopping annotate releases clicks and cannot report a stale drag", () => {
  const { mouse, report, stop } = install();
  mouse("mousedown", 10, 20);
  stop();
  mouse("mouseup", 300, 200);
  const click = mouse("click", 10, 20);
  expect(click.preventDefault).not.toHaveBeenCalled();
  expect(click.stopImmediatePropagation).not.toHaveBeenCalled();
  expect(report).not.toHaveBeenCalled();
});
