import path from "node:path";
import { describe, expect, it } from "vitest";

import {
  DESKTOP_NATIVE_BUILD_MODE,
  desktopNativeWorkbenchRuntimePlugin,
  resolveDesktopNativeWorkbenchRuntime,
} from "./vite.config";

const root = path.resolve(import.meta.dirname);
const importer = path.join(root, "src/app/store.ts");
const runtime = path.join(root, "src/ports/workbenchRuntime.desktop.ts");

describe("desktop-native runtime resolver", () => {
  it("redirects only the canonical runtime import during the desktop build", () => {
    expect(
      resolveDesktopNativeWorkbenchRuntime(
        "build",
        DESKTOP_NATIVE_BUILD_MODE,
        "../ports/workbenchRuntime",
        importer,
      ),
    ).toBe(runtime);
    expect(
      resolveDesktopNativeWorkbenchRuntime(
        "build",
        DESKTOP_NATIVE_BUILD_MODE,
        "../ports/nativeWorkbenchPorts",
        importer,
      ),
    ).toBeUndefined();
  });

  it("leaves development and non-desktop builds on the dual runtime facade", () => {
    expect(
      resolveDesktopNativeWorkbenchRuntime(
        "serve",
        DESKTOP_NATIVE_BUILD_MODE,
        "../ports/workbenchRuntime",
        importer,
      ),
    ).toBeUndefined();
    expect(
      resolveDesktopNativeWorkbenchRuntime(
        "build",
        "development",
        "../ports/workbenchRuntime",
        importer,
      ),
    ).toBeUndefined();
    expect(
      desktopNativeWorkbenchRuntimePlugin("serve", DESKTOP_NATIVE_BUILD_MODE),
    ).toBeUndefined();
  });
});
