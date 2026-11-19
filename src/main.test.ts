import { afterEach, expect, it, vi } from "vitest";

afterEach(() => {
  vi.doUnmock("./app/mountApp");
  vi.restoreAllMocks();
  document.body.replaceChildren();
  vi.resetModules();
});

// Checks only that the entry calls mountApp synchronously. The rendered
// startup ordering that visual fixtures rely on is covered in a real browser
// by the visual suite (resource, right-pane-chrome and ap-f-right-pane).
it("calls mountApp synchronously during entry evaluation", async () => {
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
