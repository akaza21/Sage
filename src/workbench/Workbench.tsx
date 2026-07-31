import { ChatSurface } from "../features/chat/ChatSurface";
import { DiffSurface } from "../features/diff/DiffSurface";
import { FileSurface } from "../features/files/FileSurface";
import { useWorkbenchStore } from "../app/store";
import { DirtyTabDialog } from "./DirtyTabDialog";
import { LeftSidebar } from "./LeftSidebar";
import { PlaceholderSurface } from "./PlaceholderSurface";
import { RightSidebar } from "./RightSidebar";
import { StatusBar } from "./StatusBar";
import { TabStrip } from "./TabStrip";

export function Workbench() {
  const project = useWorkbenchStore((state) => state.project);
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const activeTab = tabs.find((tab) => tab.id === activeTabId) ?? null;

  return (
    <main className="workbench">
      <header className="title-bar">
        <div className="title-bar__brand">
          <span className="title-bar__logo">S</span>
          <strong>Sage</strong>
        </div>
        <div className="title-bar__project">
          {project?.name ?? "No project"}
        </div>
        <div className="title-bar__spacer" />
      </header>
      <div className="workbench__body">
        <LeftSidebar />
        <div className="center-region">
          <div className="workbench__tabs">
            <TabStrip />
          </div>
          <div className="active-canvas" role="tabpanel">
            {activeTab?.kind === "chat" ? <ChatSurface /> : null}
            {activeTab?.kind === "diff" ? (
              <DiffSurface tab={activeTab} />
            ) : null}
            {activeTab?.kind === "file" ? (
              <FileSurface tab={activeTab} />
            ) : null}
            {activeTab &&
            activeTab.kind !== "chat" &&
            activeTab.kind !== "diff" &&
            activeTab.kind !== "file" ? (
              <PlaceholderSurface tab={activeTab} />
            ) : null}
            {!activeTab ? (
              <section className="placeholder-surface">
                <h1>No open tabs</h1>
                <p>
                  Use the plus button to open a chat, file, diff, or terminal.
                </p>
              </section>
            ) : null}
          </div>
        </div>
        <RightSidebar />
      </div>
      <StatusBar />
      <DirtyTabDialog />
    </main>
  );
}
