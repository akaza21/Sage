import { createNativeWorkbenchPorts } from "./nativeWorkbenchPorts";
import type { WorkbenchPorts } from "./workbenchPorts";

// This module is substituted only by the desktop-native Vite build resolver.
// Development, Vitest, and browser fixtures keep workbenchRuntime.ts, which
// selects an adapter from the actual runtime environment.
let ports: WorkbenchPorts | null = null;

export function getWorkbenchPorts(): WorkbenchPorts {
  if (!ports) {
    ports = createNativeWorkbenchPorts();
  }
  return ports;
}

export function resetWorkbenchPortsForTests(): void {
  ports = null;
}
