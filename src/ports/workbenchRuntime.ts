import { isTauriRuntime } from "./desktopRuntime";
import { createNativeWorkbenchPorts } from "./nativeWorkbenchPorts";
import type { WorkbenchPorts } from "./workbenchPorts";

// The app always talks to Sage's Rust backend: inside the desktop shell, or
// through the dev bridge that installs the same IPC surface in a browser tab.
// Playwright and Vitest swap this module for the browser simulator in
// tests/ui/simulator via the Vite `ui-fixture` resolver; nothing else does.
let ports: WorkbenchPorts | null = null;

// False only in a plain browser tab, where the native adapter has nothing
// to call.
export function hasWorkbenchBackend(): boolean {
  return isTauriRuntime();
}

export function getWorkbenchPorts(): WorkbenchPorts {
  if (!ports) {
    ports = createNativeWorkbenchPorts();
  }
  return ports;
}

export function resetWorkbenchPortsForTests(): void {
  ports = null;
}
