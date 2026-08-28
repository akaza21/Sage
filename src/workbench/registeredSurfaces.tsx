import type { WorkbenchTab } from "../domain/workbench";
import { ChatSurface } from "../features/chat/ChatSurface";
import { DiffSurface } from "../features/diff/DiffSurface";
import { ExplorerSurface } from "../features/files/ExplorerSurface";
import { FileSurface } from "../features/files/FileSurface";
import { OutlineSurface } from "../features/outline/OutlineSurface";
import { PreviewSurface } from "../features/preview/PreviewSurface";
import { ProblemsSurface } from "../features/problems/ProblemsSurface";
import { SearchSurface } from "../features/search/SearchSurface";
import { SourceControlView } from "./SourceControlView";
import { registerCanvasSurface, registerToolSurface } from "./surfaceRegistry";

function ChatCanvas({ tab }: { tab: WorkbenchTab }) {
  return <ChatSurface chatId={tab.id} />;
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
  registerToolSurface("problems", ProblemsSurface);
  registerToolSurface("outline", OutlineSurface);
  registered = true;
}
