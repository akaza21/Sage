import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { createElement } from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { registerWorkbenchSurfaces } from "./registeredSurfaces";
import { getRegisteredCanvasSurface } from "./surfaceRegistry";
import { settleLazyModules } from "../test/settleLazyModules";

afterEach(cleanup);

const workbench = path.resolve(path.dirname(fileURLToPath(import.meta.url)));
const source = (file: string) =>
  readFileSync(path.join(workbench, file), "utf8");

it("keeps all Q04 named surfaces behind dynamic imports and preserves read-only Diff attribution", async () => {
  const workbenchSource = source("Workbench.tsx");
  const registrySource = source("registeredSurfaces.tsx");
  const activeEditorCanvasSource = source("ActiveEditorCanvas.tsx");
  const chatSource = readFileSync(
    path.join(workbench, "../features/chat/acp/ChatMarkdown.tsx"),
    "utf8",
  );
  const markdownSource = readFileSync(
    path.join(workbench, "../features/files/FileMarkdownPreview.tsx"),
    "utf8",
  );
  const previewSource = readFileSync(
    path.join(workbench, "../features/preview/AgentPreviewSurface.tsx"),
    "utf8",
  );
  expect(activeEditorCanvasSource).toContain('import("./SettingsSurface")');
  expect(activeEditorCanvasSource).not.toContain('from "./SettingsSurface"');
  expect(workbenchSource).not.toContain('from "./SettingsSurface"');
  expect(workbenchSource).toContain('import("./AgentWorkbenchBody")');
  expect(workbenchSource).toContain('import("./TerminalDockShell")');
  expect(workbenchSource).not.toContain('from "./TerminalDockShell"');
  expect(source("TerminalDockShell.tsx")).toContain('import("./TerminalDock")');
  expect(workbenchSource).toContain("pinnedChatId={pinnedAgentChatOwner()}");
  expect(source("TerminalDock.tsx")).not.toContain(
    'from "../app/agentChatOwner"',
  );
  expect(workbenchSource).not.toContain('from "./AgentArtifactPane"');
  expect(workbenchSource).not.toContain('from "./AgentChatPanes"');
  expect(workbenchSource).not.toContain('from "./LeftSidebar"');
  expect(registrySource).toContain(
    'import("../features/search/SearchSurface")',
  );
  expect(registrySource).toContain('import("./SourceControlView")');
  expect(registrySource).toContain(
    'import("../features/preview/PreviewSurface")',
  );
  expect(registrySource).toContain('import("../features/diff/DiffSurface")');
  registerWorkbenchSurfaces();
  render(
    createElement(getRegisteredCanvasSurface("diff")!, {
      tab: {
        id: "working-diff",
        kind: "diff",
        title: "src/example.ts",
        position: 0,
        pinned: false,
        dirty: false,
      },
      readOnly: true,
    }),
  );
  await settleLazyModules();
  expect(
    screen.getByText(
      "Current working-tree changes against Git; may include edits from other turns.",
    ),
  ).toBeVisible();
  expect(registrySource).toContain('import("../features/files/FileSurface")');
  expect(registrySource).toContain(
    'import("../features/browser/BrowserSurface")',
  );
  expect(chatSource).not.toContain('from "./MermaidFigure"');
  expect(markdownSource).not.toContain('from "../chat/acp/MermaidFigure"');
  expect(previewSource).toContain('import("./AgentPreviewPresentation")');
});
