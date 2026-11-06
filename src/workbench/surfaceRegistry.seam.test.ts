import { render, screen } from "@testing-library/react";
import { createElement } from "react";
import { describe, expect, it, vi } from "vitest";

import type { WorkbenchTab } from "../domain/workbench";
import { settleLazyModules } from "../test/settleLazyModules";
import {
  listRegisteredCanvasKinds,
  listRegisteredToolIds,
  renderFileOrDiffCanvas,
  renderRegisteredCanvas,
  renderRegisteredTool,
  type ToolSurfaceId,
} from "./surfaceRegistry";
import { registerWorkbenchSurfaces } from "./registeredSurfaces";

// The feature surfaces themselves need the whole workbench runtime; this
// suite is about the registry, so each feature module renders what it was
// handed and the test checks the registry delivered it to the right one.
type CanvasProps = { tab: WorkbenchTab; readOnly?: boolean };
const canvas = (name: string) =>
  function Canvas({ tab, readOnly }: CanvasProps) {
    return createElement(
      "p",
      null,
      `${name} canvas for ${tab.id}${readOnly ? " (read-only)" : ""}`,
    );
  };
const tool = (name: string) =>
  function Tool() {
    return createElement("p", null, `${name} tool`);
  };

vi.mock("../features/chat/ChatSurface", () => ({
  ChatSurface: ({ chatId }: { chatId: string }) =>
    createElement("p", null, `Chat canvas for ${chatId}`),
}));
vi.mock("../features/files/FileSurface", () => ({
  default: canvas("File"),
}));
vi.mock("../features/diff/DiffSurface", () => ({
  DiffSurface: canvas("Diff"),
}));
vi.mock("../features/browser/BrowserSurface", () => ({
  BrowserSurface: canvas("Browser"),
}));
vi.mock("../features/files/ExplorerSurface", () => ({
  default: tool("Explorer"),
}));
vi.mock("../features/search/SearchSurface", () => ({
  default: tool("Search"),
}));
vi.mock("../features/outline/OutlineSurface", () => ({
  OutlineSurface: tool("Outline"),
}));
vi.mock("./SourceControlView", () => ({
  SourceControlView: tool("Source Control"),
}));
vi.mock("../features/preview/PreviewSurface", () => ({
  PreviewSurface: tool("Preview"),
}));

registerWorkbenchSurfaces();

const tab = (kind: WorkbenchTab["kind"], id: string): WorkbenchTab => ({
  id,
  kind,
  title: id,
  position: 0,
  pinned: false,
  dirty: false,
});

describe("workbench surface registry", () => {
  it("registers every canvas kind and tool the workbench lays out", () => {
    expect(listRegisteredCanvasKinds().sort()).toEqual([
      "browser",
      "chat",
      "diff",
      "file",
    ]);
    expect(listRegisteredToolIds().sort()).toEqual([
      "explorer",
      "outline",
      "preview",
      "search",
      "sourceControl",
    ]);
  });

  it.each([
    ["chat", "chat:7", "Chat canvas for chat:7"],
    ["file", "file:src/main.ts", "File canvas for file:src/main.ts"],
    ["diff", "diff:src/main.ts", "Diff canvas for diff:src/main.ts"],
    ["browser", "browser:1", "Browser canvas for browser:1"],
  ] as const)(
    "opens a %s tab in its own surface with that tab",
    async (kind, id, text) => {
      render(renderRegisteredCanvas(kind, tab(kind, id)));
      await settleLazyModules();
      expect(await screen.findByText(text)).toBeInTheDocument();
    },
  );

  it("hands read-only through to the file and diff surfaces and ignores other tabs", async () => {
    render(
      createElement(
        "div",
        null,
        renderFileOrDiffCanvas(tab("file", "file:a.ts"), { readOnly: true }),
        renderFileOrDiffCanvas(tab("diff", "diff:b.ts"), { readOnly: true }),
      ),
    );
    await settleLazyModules();
    expect(
      await screen.findByText("File canvas for file:a.ts (read-only)"),
    ).toBeInTheDocument();
    expect(
      await screen.findByText("Diff canvas for diff:b.ts (read-only)"),
    ).toBeInTheDocument();
    expect(renderFileOrDiffCanvas(tab("chat", "chat:1"))).toBeNull();
  });

  it.each([
    ["explorer", "Explorer tool"],
    ["search", "Search tool"],
    ["sourceControl", "Source Control tool"],
    ["preview", "Preview tool"],
    ["outline", "Outline tool"],
  ] as const satisfies ReadonlyArray<readonly [ToolSurfaceId, string]>)(
    "opens the %s tool in its own surface",
    async (id, text) => {
      render(renderRegisteredTool(id));
      await settleLazyModules();
      expect(await screen.findByText(text)).toBeInTheDocument();
    },
  );
});
