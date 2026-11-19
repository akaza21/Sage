import { afterEach, expect, it, vi } from "vitest";

afterEach(() => {
  vi.doUnmock("./devBridge/installBrowserBridge");
  vi.doUnmock("./app/installWorkbenchNavigationGuard");
  vi.doUnmock("./app/mountApp");
  document.body.replaceChildren();
  history.replaceState(null, "", "/");
  vi.resetModules();
});

it("mounts synchronously during entry evaluation without a bridge query", async () => {
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

it("removes the pairing credential before a dynamically imported module can observe the URL", async () => {
  const code = "a".repeat(64);
  history.replaceState(null, "", `/?bridge=32123#${code}`);
  document.body.innerHTML = '<div id="root"></div>';
  let importedHash: string | undefined;
  let installedCode: string | undefined;
  vi.doMock("./devBridge/installBrowserBridge", () => {
    importedHash = location.hash;
    return {
      installBrowserBridge: async (credential: string) => {
        installedCode = credential;
      },
    };
  });
  vi.doMock("./app/installWorkbenchNavigationGuard", () => ({
    installWorkbenchNavigationGuard: () => () => {},
  }));
  let appGraphHash: string | undefined;
  vi.doMock("./app/mountApp", () => {
    appGraphHash = location.hash;
    return { mountApp: () => {} };
  });
  await import("./main");
  await vi.waitFor(() => expect(installedCode).toBe(code));
  expect(appGraphHash).toBe("");
  expect(importedHash).toBe("");
  expect(location.href).not.toContain(code);
});
