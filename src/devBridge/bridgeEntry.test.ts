import { afterEach, expect, it, vi } from "vitest";

import { isTauriRuntime } from "../ports/desktopRuntime";

const code = "a".repeat(64);

afterEach(() => {
  vi.doUnmock("./installBrowserBridge");
  vi.doUnmock("../app/installWorkbenchNavigationGuard");
  vi.doUnmock("../app/mountApp");
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  document.body.replaceChildren();
  history.replaceState(null, "", "/");
  vi.resetModules();
});

function startEntry(install: (credential: string) => Promise<void>) {
  history.replaceState(null, "", `/?bridge=32123#${code}`);
  document.body.innerHTML = '<div id="root"></div>';
  const seen = {
    bridgeModuleHash: undefined as string | undefined,
    appGraphHash: undefined as string | undefined,
    appGraphSawNativeRuntime: undefined as boolean | undefined,
    mounted: false,
  };
  vi.doMock("./installBrowserBridge", () => {
    seen.bridgeModuleHash = location.hash;
    return { installBrowserBridge: install };
  });
  vi.doMock("../app/installWorkbenchNavigationGuard", () => ({
    installWorkbenchNavigationGuard: () => () => {},
  }));
  vi.doMock("../app/mountApp", () => {
    // Module-level runtime detection in the app graph (workbench ports)
    // happens here, at evaluation.
    seen.appGraphHash = location.hash;
    seen.appGraphSawNativeRuntime = isTauriRuntime();
    return {
      mountApp: () => {
        seen.mounted = true;
      },
    };
  });
  return { seen, loaded: import("./bridgeEntry") };
}

it("removes the pairing credential before any other module can observe the URL", async () => {
  let installedCode: string | undefined;
  const { seen, loaded } = startEntry(async (credential) => {
    installedCode = credential;
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
  });
  await loaded;
  await vi.waitFor(() => expect(seen.mounted).toBe(true));

  expect(installedCode).toBe(code);
  expect(seen.bridgeModuleHash).toBe("");
  expect(seen.appGraphHash).toBe("");
  expect(location.href).not.toContain(code);
});

it("evaluates the app graph only after the bridge provides the native runtime", async () => {
  const { seen, loaded } = startEntry(async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
  });
  await loaded;
  await vi.waitFor(() => expect(seen.mounted).toBe(true));

  expect(seen.appGraphSawNativeRuntime).toBe(true);
});

it("shows the pairing failure and never loads the app when the bridge refuses", async () => {
  const { seen, loaded } = startEntry(async () => {
    throw new Error("Open the pairing link from Sage");
  });
  await loaded;
  const root = document.getElementById("root")!;
  await vi.waitFor(() =>
    expect(root.textContent).toBe("Open the pairing link from Sage"),
  );

  expect(seen.appGraphSawNativeRuntime).toBeUndefined();
  expect(seen.mounted).toBe(false);
});
