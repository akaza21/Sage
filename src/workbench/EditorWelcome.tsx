import { Files, MessageSquare, Search } from "../ui/leanIcons";

import { useWorkbenchStore, type SidebarView } from "../app/store";

export function EditorWelcome({ onOpenAgent }: { onOpenAgent: () => void }) {
  const setSidebarView = useWorkbenchStore((state) => state.setSidebarView);

  const openView = (view: SidebarView) => setSidebarView(view);

  return (
    <section className="editor-welcome" aria-label="Editor welcome">
      <div className="editor-welcome__mark" aria-hidden="true">
        S
      </div>
      <div className="editor-welcome__actions">
        <button type="button" onClick={() => openView("explorer")}>
          <Files size={16} aria-hidden="true" />
          Browse files
        </button>
        <button type="button" onClick={() => openView("search")}>
          <Search size={16} aria-hidden="true" />
          Search files
        </button>
        <button type="button" onClick={onOpenAgent}>
          <MessageSquare size={16} aria-hidden="true" />
          Open Agent
        </button>
      </div>
      <p>Use Settings to tune how every File is displayed.</p>
    </section>
  );
}
