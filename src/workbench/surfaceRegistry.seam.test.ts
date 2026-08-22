import { readFile } from "node:fs/promises";
import path from "node:path";
import { createElement, type ReactNode } from "react";
import { describe, expect, it } from "vitest";

import { ExplorerSurface } from "../features/files/ExplorerSurface";
import {
  getRegisteredCanvasSurface,
  getRegisteredToolSurface,
  listRegisteredCanvasKinds,
  listRegisteredToolIds,
} from "./surfaceRegistry";
import { registerWorkbenchSurfaces } from "./registeredSurfaces";

registerWorkbenchSurfaces();

const srcRoot = path.resolve(import.meta.dirname, "..");

describe("S0.4 composition seam", () => {
  it("owns Explorer as a feature surface instead of an inline RightSidebar view", () => {
    expect(typeof ExplorerSurface).toBe("function");
  });

  it("registers canvas and tool surfaces so Workbench.tsx is not the feature list", () => {
    expect(listRegisteredCanvasKinds().sort()).toEqual([
      "chat",
      "diff",
      "file",
    ]);
    expect(listRegisteredToolIds()).toContain("explorer");
    expect(getRegisteredCanvasSurface("chat")).toBeTypeOf("function");
    expect(getRegisteredCanvasSurface("file")).toBeTypeOf("function");
    expect(getRegisteredCanvasSurface("diff")).toBeTypeOf("function");
    const explorer = getRegisteredToolSurface("explorer");
    expect(explorer).toBeTypeOf("function");
    const node: ReactNode = createElement(explorer!);
    expect(node).toBeTruthy();
  });

  it("keeps Workbench.tsx free of feature surface imports", async () => {
    const source = await readFile(
      path.join(srcRoot, "workbench/Workbench.tsx"),
      "utf8",
    );
    expect(source).not.toMatch(
      /from ["']\.\.\/features\/chat\/ChatSurface["']/,
    );
    expect(source).not.toMatch(
      /from ["']\.\.\/features\/diff\/DiffSurface["']/,
    );
    expect(source).not.toMatch(
      /from ["']\.\.\/features\/files\/FileSurface["']/,
    );
    expect(source).toMatch(/registeredSurfaces|surfaceRegistry/);
  });

  it("splits Explorer surface CSS from shell tokens", async () => {
    const [surfaceCss, globalCss, sidebar] = await Promise.all([
      readFile(
        path.join(srcRoot, "features/files/explorer-surface.css"),
        "utf8",
      ),
      readFile(path.join(srcRoot, "styles/global.css"), "utf8"),
      readFile(path.join(srcRoot, "workbench/RightSidebar.tsx"), "utf8"),
    ]);
    expect(surfaceCss).toMatch(/\.tree__/);
    expect(surfaceCss).toMatch(/\.root-tree/);
    expect(globalCss).not.toMatch(/^\.root-tree\b/m);
    expect(globalCss).not.toMatch(/^\.tree__row\b/m);
    expect(sidebar).not.toMatch(/function ExplorerView/);
    expect(sidebar).not.toMatch(/function RootTree/);
  });
});
