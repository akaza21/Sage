import { useState } from "react";

import type { WorkbenchTab } from "../domain/workbench";
import type * as ExplorerModule from "../features/files/ExplorerSurface";
import { LazyModuleBoundary } from "../ui/LazyModuleBoundary";
import {
  registerCanvasSurface,
  registerToolSurface,
  type CanvasSurfaceOptions,
} from "./surfaceRegistry";

// Explorer starts loading with the workbench but stays out of its startup
// bundle, which the Agent window shares. Once loaded it renders directly, so
// the Editor shows no loading state after the first moment.
let explorerModule: typeof ExplorerModule | null = null;
const loadExplorerSurface = () =>
  import("../features/files/ExplorerSurface").then((module) => {
    explorerModule = module;
    return module;
  });
const loadChatSurface = () =>
  import("../features/chat/ChatSurface").then(({ ChatSurface }) => ({
    default: ChatSurface,
  }));
const loadSearchSurface = () => import("../features/search/SearchSurface");
const loadOutlineSurface = () =>
  import("../features/outline/OutlineSurface").then(({ OutlineSurface }) => ({
    default: OutlineSurface,
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
const loadFileSurface = () => import("../features/files/FileSurface");
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
  label,
  load,
  ...options
}: {
  tab: WorkbenchTab;
  label: string;
  load: () => Promise<{
    default: React.ComponentType<{ tab: WorkbenchTab } & CanvasSurfaceOptions>;
  }>;
} & CanvasSurfaceOptions) {
  return (
    <LazyModuleBoundary
      label={label}
      load={load}
      render={(Surface) => <Surface tab={tab} {...options} />}
    />
  );
}

function ChatCanvas({ tab }: { tab: WorkbenchTab }) {
  return (
    <LazyModuleBoundary<{ chatId: string }>
      label="Chat"
      load={loadChatSurface}
      render={(Surface) => <Surface chatId={tab.id} />}
    />
  );
}

function ExplorerToolSurface() {
  // Chosen once per mount, so Explorer never remounts (losing an open menu or
  // dialog) when its module arrives while the fallback is showing.
  const [loaded] = useState(() => explorerModule);
  return loaded ? (
    <loaded.default />
  ) : (
    <LazyToolSurface label="Explorer" load={loadExplorerSurface} />
  );
}

let registered = false;

export function registerWorkbenchSurfaces(): void {
  if (registered) return;
  registerCanvasSurface("chat", ChatCanvas);
  registerCanvasSurface("diff", ({ tab, readOnly, hostHeader }) => (
    <LazyCanvasSurface
      tab={tab}
      readOnly={readOnly}
      hostHeader={hostHeader}
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
  // A failed early load shows its retry when Explorer is opened.
  void loadExplorerSurface().catch(() => undefined);
  registerToolSurface("explorer", ExplorerToolSurface);
  registerToolSurface("search", () => (
    <LazyToolSurface label="Search" load={loadSearchSurface} />
  ));
  registerToolSurface("sourceControl", () => (
    <LazyToolSurface label="Source Control" load={loadSourceControlView} />
  ));
  registerToolSurface("preview", () => (
    <LazyToolSurface label="Preview" load={loadPreviewSurface} />
  ));
  registerToolSurface("outline", () => (
    <LazyToolSurface label="Outline" load={loadOutlineSurface} />
  ));
  registered = true;
}
