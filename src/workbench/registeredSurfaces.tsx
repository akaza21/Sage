import type { WorkbenchTab } from "../domain/workbench";
import { ChatSurface } from "../features/chat/ChatSurface";
import { DiffSurface } from "../features/diff/DiffSurface";
import { ExplorerSurface } from "../features/files/ExplorerSurface";
import { FileSurface } from "../features/files/FileSurface";
import { SearchSurface } from "../features/search/SearchSurface";
import { PreviewSurface } from "../features/preview/PreviewSurface";
import { SourceControlView } from "./SourceControlView";
import { registerCanvasSurface, registerToolSurface } from "./surfaceRegistry";

function ChatCanvas({ tab: _tab }: { tab: WorkbenchTab }) {
  return <ChatSurface />;
}

let registered = false;

export function registerWorkbenchSurfaces(): void {
  if (registered) return;
  registerCanvasSurface("chat", ChatCanvas);
  registerCanvasSurface("diff", DiffSurface);
  registerCanvasSurface("file", FileSurface);
  registerToolSurface("explorer", ExplorerSurface);
  registerToolSurface("search", SearchSurface);
  registerToolSurface("sourceControl", SourceControlView);
  registerToolSurface("preview", PreviewSurface);
  registered = true;
}
