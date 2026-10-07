import { createElement, type ComponentType, type ReactNode } from "react";

import type { WorkbenchTab } from "../domain/workbench";

export type CanvasSurfaceKind = "browser" | "chat" | "diff" | "file";
export type ToolSurfaceId =
  "explorer" | "search" | "sourceControl" | "preview" | "problems" | "outline";

type CanvasSurface = ComponentType<{
  tab: WorkbenchTab;
  readOnly?: boolean;
  paneHeader?: ReactNode;
}>;
type ToolSurface = ComponentType;

const canvasSurfaces = new Map<CanvasSurfaceKind, CanvasSurface>();
const toolSurfaces = new Map<ToolSurfaceId, ToolSurface>();

export function registerCanvasSurface(
  kind: CanvasSurfaceKind,
  surface: CanvasSurface,
): void {
  canvasSurfaces.set(kind, surface);
}

export function registerToolSurface(
  id: ToolSurfaceId,
  surface: ToolSurface,
): void {
  toolSurfaces.set(id, surface);
}

export function getRegisteredCanvasSurface(
  kind: CanvasSurfaceKind,
): CanvasSurface | undefined {
  return canvasSurfaces.get(kind);
}

export function getRegisteredToolSurface(
  id: ToolSurfaceId,
): ToolSurface | undefined {
  return toolSurfaces.get(id);
}

export function listRegisteredCanvasKinds(): CanvasSurfaceKind[] {
  return [...canvasSurfaces.keys()];
}

export function listRegisteredToolIds(): ToolSurfaceId[] {
  return [...toolSurfaces.keys()];
}

export function renderRegisteredCanvas(
  kind: CanvasSurfaceKind,
  tab: WorkbenchTab,
  options: { paneHeader?: ReactNode } = {},
): ReactNode {
  const Surface = canvasSurfaces.get(kind);
  return Surface ? createElement(Surface, { tab, ...options }) : null;
}

export function renderFileOrDiffCanvas(
  tab: WorkbenchTab,
  options: { readOnly?: boolean } = {},
): ReactNode {
  if (tab.kind !== "file" && tab.kind !== "diff") {
    return null;
  }
  const Surface = canvasSurfaces.get(tab.kind);
  return Surface ? createElement(Surface, { tab, ...options }) : null;
}

export function renderRegisteredTool(id: ToolSurfaceId): ReactNode {
  const Surface = toolSurfaces.get(id);
  return Surface ? createElement(Surface) : null;
}
