import { afterEach, expect, it, vi } from "vitest";

afterEach(() => {
  vi.doUnmock("./app/mountApp");
  vi.restoreAllMocks();
  document.body.replaceChildren();
  vi.resetModules();
});

// Visual fixtures and the native shell rely on the app being mounted by the
// time the entry module has evaluated, before the page's load event.
it("mounts synchronously during entry evaluation", async () => {
  document.body.innerHTML = '<div id="root"></div>';
  const root = document.getElementById("root")!;
  let contentAfterEntry: string | null | undefined;
  vi.spyOn(document, "getElementById").mockImplementationOnce(() => {
    // Observe the first microtask after entry evaluation starts, before any
    // awaited mount could resume (even an already resolved promise).
    queueMicrotask(() => {
      contentAfterEntry = root.textContent;
    });
    return root;
  });
  vi.doMock("./app/mountApp", () => ({
    mountApp: (root: HTMLElement) => {
      root.textContent = "Sage mounted";
    },
  }));

  await import("./main");

  expect(contentAfterEntry).toBe("Sage mounted");
});
