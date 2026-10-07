import type { ReactNode } from "react";

import type { WorkbenchTab } from "../domain/workbench";
import { ChatSurface } from "../features/chat/ChatSurface";
import { ExplorerSurface } from "../features/files/ExplorerSurface";
import { OutlineSurface } from "../features/outline/OutlineSurface";
import { ProblemsSurface } from "../features/problems/ProblemsSurface";
import { LazyModuleBoundary } from "../ui/LazyModuleBoundary";
import { registerCanvasSurface, registerToolSurface } from "./surfaceRegistry";

const loadSearchSurface = () =>
  import("../features/search/SearchSurface").then(({ SearchSurface }) => ({
    default: SearchSurface,
  }));
const loadSourceControlView = () =>
  import("./SourceControlView").then(({ SourceControlView }) => ({
    default: SourceControlView,
  }));
const loadPreviewSurface = () =>
  import("../features/preview/PreviewSurface").then(({ PreviewSurface }) => ({
    default: PreviewSurface,
  }));
const loadDiffSurface = () =>
  import("../features/diff/DiffSurface").then(({ DiffSurface }) => ({
    default: DiffSurface,
  }));
const loadFileSurface = () =>
  import("../features/files/FileSurface").then(({ FileSurface }) => ({
    default: FileSurface,
  }));
const loadBrowserSurface = () =>
  import("../features/browser/BrowserSurface").then(({ BrowserSurface }) => ({
    default: BrowserSurface,
  }));

function LazyToolSurface({
  label,
  load,
}: {
  label: string;
  load: () => Promise<{ default: React.ComponentType }>;
}) {
  return (
    <LazyModuleBoundary
      label={label}
      load={load}
      render={(Surface) => <Surface />}
    />
  );
}

function LazyCanvasSurface({
  tab,
  readOnly,
  label,
  load,
}: {
  tab: WorkbenchTab;
  readOnly?: boolean;
  label: string;
  load: () => Promise<{
    default: React.ComponentType<{ tab: WorkbenchTab; readOnly?: boolean }>;
  }>;
}) {
  return (
    <LazyModuleBoundary
      label={label}
      load={load}
      render={(Surface) => <Surface tab={tab} readOnly={readOnly} />}
    />
  );
}

function ChatCanvas({
  tab,
  paneHeader,
}: {
  tab: WorkbenchTab;
  paneHeader?: ReactNode;
}) {
  return <ChatSurface chatId={tab.id} paneHeader={paneHeader} />;
}

let registered = false;

export function registerWorkbenchSurfaces(): void {
  if (registered) return;
  registerCanvasSurface("chat", ChatCanvas);
  registerCanvasSurface("diff", ({ tab, readOnly }) => (
    <LazyCanvasSurface
      tab={tab}
      readOnly={readOnly}
      label="Diff"
      load={loadDiffSurface}
    />
  ));
  registerCanvasSurface("file", ({ tab, readOnly }) => (
    <LazyCanvasSurface
      tab={tab}
      readOnly={readOnly}
      label="File"
      load={loadFileSurface}
    />
  ));
  registerCanvasSurface("browser", ({ tab }) => (
    <LazyCanvasSurface tab={tab} label="Browser" load={loadBrowserSurface} />
  ));
  registerToolSurface("explorer", ExplorerSurface);
  registerToolSurface("search", () => (
    <LazyToolSurface label="Search" load={loadSearchSurface} />
  ));
  registerToolSurface("sourceControl", () => (
    <LazyToolSurface label="Source Control" load={loadSourceControlView} />
  ));
  registerToolSurface("preview", () => (
    <LazyToolSurface label="Preview" load={loadPreviewSurface} />
  ));
  registerToolSurface("problems", ProblemsSurface);
  registerToolSurface("outline", OutlineSurface);
  registered = true;
}
