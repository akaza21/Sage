import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";
import path from "node:path";
import { fileURLToPath } from "node:url";
import type { Plugin } from "vite";

const host = process.env.TAURI_DEV_HOST;
const sourceMaps = process.env.SAGE_BUILD_SOURCEMAPS === "1";
const root = path.dirname(fileURLToPath(import.meta.url));
const workbenchRuntime = path.join(root, "src/ports/workbenchRuntime.ts");
const desktopWorkbenchRuntime = path.join(
  root,
  "src/ports/workbenchRuntime.desktop.ts",
);

export const DESKTOP_NATIVE_BUILD_MODE = "desktop-native";

export function resolveDesktopNativeWorkbenchRuntime(
  command: string,
  mode: string,
  source: string,
  importer?: string,
): string | undefined {
  if (command !== "build" || mode !== DESKTOP_NATIVE_BUILD_MODE) {
    return undefined;
  }
  const resolved =
    source.startsWith(".") && importer
      ? path.resolve(path.dirname(importer), source)
      : path.resolve(source);
  if (resolved === workbenchRuntime || `${resolved}.ts` === workbenchRuntime) {
    return desktopWorkbenchRuntime;
  }
  return undefined;
}

export function desktopNativeWorkbenchRuntimePlugin(
  command: string,
  mode: string,
): Plugin | undefined {
  if (command !== "build" || mode !== DESKTOP_NATIVE_BUILD_MODE) {
    return undefined;
  }
  return {
    name: "sage-desktop-native-workbench-runtime",
    enforce: "pre",
    resolveId(source, importer) {
      return resolveDesktopNativeWorkbenchRuntime(
        command,
        mode,
        source,
        importer,
      );
    },
    generateBundle() {
      this.emitFile({
        type: "asset",
        fileName: ".sage-web-build-target.json",
        source: `${JSON.stringify({ schemaVersion: 1, target: DESKTOP_NATIVE_BUILD_MODE })}\n`,
      });
    },
  };
}

export default defineConfig(({ command, mode }) => ({
  cacheDir: process.env.SAGE_VITE_CACHE_DIR,
  plugins: [react(), desktopNativeWorkbenchRuntimePlugin(command, mode)],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host ?? "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    manifest: true,
    modulePreload: false,
    target: "es2022",
    sourcemap: sourceMaps,
    reportCompressedSize: true,
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    restoreMocks: true,
    clearMocks: true,
  },
}));
