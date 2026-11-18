import { afterEach, expect, it, vi } from "vitest";

afterEach(() => {
  vi.doUnmock("./devBridge/installBrowserBridge");
  vi.doUnmock("./app/installWorkbenchNavigationGuard");
  vi.doUnmock("./main.desktop");
  document.body.replaceChildren();
  history.replaceState(null, "", "/");
  vi.resetModules();
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
  vi.doMock("./main.desktop", () => ({}));
  await import("./main");
  await vi.waitFor(() => expect(installedCode).toBe(code));
  expect(importedHash).toBe("");
  expect(location.href).not.toContain(code);
});
