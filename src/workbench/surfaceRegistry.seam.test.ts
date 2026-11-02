import { createElement, type ReactNode } from "react";
import { describe, expect, it } from "vitest";

import {
  getRegisteredCanvasSurface,
  getRegisteredToolSurface,
  listRegisteredCanvasKinds,
  listRegisteredToolIds,
} from "./surfaceRegistry";
import { registerWorkbenchSurfaces } from "./registeredSurfaces";

registerWorkbenchSurfaces();

describe("S0.4 composition seam", () => {
  it("registers canvas and tool surfaces so Workbench.tsx is not the feature list", () => {
    expect(listRegisteredCanvasKinds().sort()).toEqual([
      "browser",
      "chat",
      "diff",
      "file",
    ]);
    expect(listRegisteredToolIds()).toContain("explorer");
    expect(getRegisteredCanvasSurface("chat")).toBeTypeOf("function");
    expect(getRegisteredCanvasSurface("file")).toBeTypeOf("function");
    expect(getRegisteredCanvasSurface("diff")).toBeTypeOf("function");
    expect(getRegisteredCanvasSurface("browser")).toBeTypeOf("function");
    const explorer = getRegisteredToolSurface("explorer");
    expect(explorer).toBeTypeOf("function");
    const node: ReactNode = createElement(explorer!);
    expect(node).toBeTruthy();
  });
});
