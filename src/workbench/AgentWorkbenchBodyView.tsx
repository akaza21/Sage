import { useWorkbenchStore } from "../app/store";
import { useEffect, useRef, type CSSProperties, type ReactNode } from "react";
import type { PreviewTargetRef } from "../domain/previewProjection";

import { AgentChatOwnerScopeContext } from "../app/agentChatOwner";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import { useRetainedElement } from "../ui/useRetainedElement";
import { AgentArtifactPane } from "./AgentArtifactPane";
import { AgentChatPanes } from "./AgentChatPanes";
import { AgentToolsProvider } from "./AgentToolsProvider";
import { LeftSidebar } from "./LeftSidebar";
import { WorkbenchSidebarSplitter } from "./WorkbenchSidebarSplitter";
import { useNavigationFocusCarry } from "./useNavigationFocusCarry";
import { AgentNavigationPart } from "./agentNavigationLazy";
import {
  AgentHomeNavigationContext,
  navigateAgentHome,
} from "./agentRailNavigation";
import type { agentWorkbenchLayout } from "./agentWorkbenchLayout";

export default function AgentWorkbenchBodyView({
  layout,
  style,
  drawerOpen,
  navigationCollapsed = false,
  onExpandNavigation,
  artifactExpanded,
  artifactMaximized,
  centerVisible,
  onArtifactMaximizedChange,
  onArtifactExpandedChange,
  onAgentLeftRailWidthChange,
  onAgentArtifactWidthChange,
  onOpenArtifactInEditor,
  onOpenTerminal,
  onPreviewTargetChange,
  bottomPanel,
}: {
  layout: ReturnType<typeof agentWorkbenchLayout>;
  style: CSSProperties | undefined;
  drawerOpen: boolean;
  navigationCollapsed?: boolean;
  onExpandNavigation: () => void;
  artifactExpanded: boolean;
  artifactMaximized: boolean;
  centerVisible: boolean;
  onArtifactMaximizedChange: (maximized: boolean) => void;
  onArtifactExpandedChange: (expanded: boolean) => void;
  onAgentLeftRailWidthChange: (width: number) => void;
  onAgentArtifactWidthChange: (width: number) => void;
  onOpenArtifactInEditor: (tabId: string) => void;
  onOpenTerminal?: () => void;
  bottomPanel?: ReactNode;
  onPreviewTargetChange?: (target: PreviewTargetRef | null) => void;
}) {
  const backgroundInert =
    layout.artifactModal || (artifactExpanded && artifactMaximized);
  const openSettings = useAgentSettingsState((state) => state.open);
  const [artifactHeader, setArtifactHeader] =
    useRetainedElement<HTMLDivElement>();
  const railRef = useNavigationFocusCarry(navigationCollapsed);
  const draftShown = useWorkbenchStore(
    (state) => state.agentCenter === "draft",
  );
  useEffect(() => {
    if (draftShown && layout.artifactOverlay) onArtifactExpandedChange(false);
  }, [draftShown, layout.artifactOverlay, onArtifactExpandedChange]);
  const homeFocusPending = useRef(false);
  useEffect(() => {
    if (!homeFocusPending.current || drawerOpen) return;
    homeFocusPending.current = false;
    navigateAgentHome();
  }, [drawerOpen]);
  const navigateHome = () => {
    if (layout.artifactOverlay) {
      homeFocusPending.current = true;
      onArtifactExpandedChange(false);
    } else navigateAgentHome();
  };
  return (
    <AgentChatOwnerScopeContext.Provider value>
      <AgentToolsProvider onOpenTerminal={onOpenTerminal}>
        <div
          className={`workbench__body${layout.artifactOverlay ? " workbench__body--artifact-overlay" : ""}`}
          style={
            {
              ...style,
              "--agent-artifact-overlay-width": `${layout.artifactOverlayWidth}px`,
            } as CSSProperties
          }
        >
          <AgentHomeNavigationContext.Provider value={navigateHome}>
            {navigationCollapsed ? (
              <AgentNavigationPart
                part="AgentCollapsedNavigation"
                fallback={
                  <div className="project-sidebar" aria-hidden="true" />
                }
                inert={backgroundInert}
                onOpenSettings={() => openSettings()}
                onExpand={onExpandNavigation}
                railRef={railRef}
              />
            ) : (
              <LeftSidebar inert={backgroundInert} railRef={railRef} />
            )}
          </AgentHomeNavigationContext.Provider>
          {layout.navigationResizable ? (
            <WorkbenchSidebarSplitter
              ariaLabel="Resize project and chat rail"
              side="leading"
              value={layout.leftRailWidth}
              width={layout.leftRailBounds}
              onChange={onAgentLeftRailWidthChange}
            />
          ) : null}
          <div
            className="center-region"
            inert={!centerVisible}
            aria-hidden={!centerVisible || undefined}
          >
            <AgentChatPanes
              visible={centerVisible}
              compact={layout.compact}
              navigationCollapsed={navigationCollapsed}
              onArtifactHeaderMount={setArtifactHeader}
            />
            {bottomPanel}
          </div>
          {!layout.artifactOverlay && artifactExpanded ? (
            <WorkbenchSidebarSplitter
              ariaLabel="Resize files and preview"
              side="trailing"
              value={layout.artifactWidth}
              width={layout.artifactBounds}
              onChange={onAgentArtifactWidthChange}
            />
          ) : null}
          {layout.artifactModal ? (
            <button
              className="agent-artifact-backdrop"
              type="button"
              tabIndex={-1}
              aria-label="Dismiss files and preview"
              onClick={() => onArtifactExpandedChange(false)}
            />
          ) : null}
          <AgentArtifactPane
            onPreviewTargetChange={onPreviewTargetChange}
            maximized={artifactMaximized}
            onMaximizedChange={onArtifactMaximizedChange}
            expanded={artifactExpanded}
            focusOnExpand={layout.artifactOverlay}
            modal={layout.artifactModal}
            overlayOnExpand={layout.artifactOverlayOnExpand}
            headerNode={artifactHeader}
            onExpandedChange={onArtifactExpandedChange}
            onOpenInEditor={onOpenArtifactInEditor}
          />
        </div>
      </AgentToolsProvider>
    </AgentChatOwnerScopeContext.Provider>
  );
}
