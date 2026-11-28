import {
  AGENT_ICON_RAIL_WIDTH,
  AGENT_ARTIFACT_WIDTH,
  AGENT_CHAT_MIN_WIDTH,
  AGENT_LEFT_RAIL_WIDTH,
  AGENT_SIDEBAR_AUTO_HIDE_BELOW,
  agentDesktopArtifactWidth,
  agentDesktopLeftRailWidth,
  clampWorkbenchSidebarWidth,
  fitAgentDesktopSidebarWidths,
  initialAgentDesktopSidebarWidths,
  type AgentDesktopSidebarPriority,
  type AgentDesktopSidebarWidths,
} from "./workbenchSidebarResize";

const COMPACT_BELOW = 761;
const INLINE_RESOURCE_SPAN =
  AGENT_CHAT_MIN_WIDTH + 16 + AGENT_ARTIFACT_WIDTH.minWidth;
const INLINE_RESOURCE_MIN =
  AGENT_LEFT_RAIL_WIDTH.defaultWidth + INLINE_RESOURCE_SPAN;

export function agentWorkbenchLayout({
  viewportWidth,
  artifactExpanded,
  navigationCollapsed: navigationCollapsedRequest = false,
  navigationOverlayOpen = false,
  requestedWidths,
  prioritize = "left",
}: {
  viewportWidth: number;
  artifactExpanded: boolean;
  navigationCollapsed?: boolean;
  /** Under the auto-hide width, whether the toggle has the panel out. */
  navigationOverlayOpen?: boolean;
  requestedWidths?: Partial<AgentDesktopSidebarWidths>;
  prioritize?: AgentDesktopSidebarPriority;
}) {
  const desktopWidth = Math.max(INLINE_RESOURCE_MIN, viewportWidth);
  // Under 800 px the panel never docks, so it never squeezes the chat.
  const navigationAutoHidden = viewportWidth < AGENT_SIDEBAR_AUTO_HIDE_BELOW;
  const desktop = !navigationAutoHidden;
  const defaults = initialAgentDesktopSidebarWidths(desktopWidth);
  const requested = {
    leftRailWidth: requestedWidths?.leftRailWidth ?? defaults.leftRailWidth,
    artifactWidth: requestedWidths?.artifactWidth ?? defaults.artifactWidth,
  };
  const artifactShare =
    requestedWidths?.artifactWidth ?? Math.round(viewportWidth * 0.45);
  const expandedNavigationBounds = {
    ...AGENT_LEFT_RAIL_WIDTH,
    maxWidth: Math.max(
      AGENT_LEFT_RAIL_WIDTH.minWidth,
      Math.min(
        AGENT_LEFT_RAIL_WIDTH.maxWidth,
        viewportWidth - AGENT_CHAT_MIN_WIDTH - 8,
      ),
    ),
  };
  // Once the person drags either splitter, the allocator trades width between
  // the rail and the Files by resize priority, so a drag never folds the rail
  // it is sizing.
  const filesRoomBesideNavigation =
    viewportWidth -
    clampWorkbenchSidebarWidth(
      expandedNavigationBounds,
      requested.leftRailWidth,
    ) -
    AGENT_CHAT_MIN_WIDTH -
    16;
  const navigationFits =
    desktop &&
    filesRoomBesideNavigation >=
      (requestedWidths?.artifactWidth === undefined &&
      requestedWidths?.leftRailWidth === undefined
        ? Math.max(AGENT_ARTIFACT_WIDTH.minWidth, artifactShare)
        : AGENT_ARTIFACT_WIDTH.minWidth);
  const navigationCollapsed =
    navigationCollapsedRequest ||
    navigationAutoHidden ||
    (artifactExpanded && !navigationFits);
  const navigationResizable = !navigationCollapsed;
  const navigationBounds = expandedNavigationBounds;
  const artifactInlineAvailable =
    navigationCollapsedRequest || !navigationFits
      ? viewportWidth - AGENT_ICON_RAIL_WIDTH >= INLINE_RESOURCE_SPAN
      : true;
  const artifactInline = artifactExpanded && artifactInlineAvailable;
  const leftRailBounds = artifactInline
    ? agentDesktopLeftRailWidth(viewportWidth)
    : navigationBounds;
  const artifactBounds =
    navigationCollapsed && artifactInline
      ? {
          ...AGENT_ARTIFACT_WIDTH,
          maxWidth: Math.min(
            AGENT_ARTIFACT_WIDTH.maxWidth,
            viewportWidth - AGENT_ICON_RAIL_WIDTH - AGENT_CHAT_MIN_WIDTH - 8,
          ),
        }
      : agentDesktopArtifactWidth(
          artifactInline ? viewportWidth : desktopWidth,
        );
  const widths = navigationCollapsed
    ? {
        leftRailWidth: AGENT_ICON_RAIL_WIDTH,
        artifactWidth: clampWorkbenchSidebarWidth(
          artifactBounds,
          artifactShare,
        ),
      }
    : artifactInline
      ? fitAgentDesktopSidebarWidths({
          viewportWidth,
          ...requested,
          prioritize,
        })
      : {
          leftRailWidth: clampWorkbenchSidebarWidth(
            leftRailBounds,
            requested.leftRailWidth,
          ),
          artifactWidth: clampWorkbenchSidebarWidth(
            artifactBounds,
            requested.artifactWidth,
          ),
        };
  const leadingSplitter = navigationResizable ? 8 : 0;
  const chatWidth =
    viewportWidth -
    widths.leftRailWidth -
    leadingSplitter -
    (artifactInline ? widths.artifactWidth + 8 : 0);
  const artifactOverlay = artifactExpanded && !artifactInline;
  const artifactModal = artifactOverlay && chatWidth < 320;
  const artifactOverlayWidth = artifactModal
    ? Math.min(420, viewportWidth - 16)
    : chatWidth;
  const navigationOverlay = navigationAutoHidden && navigationOverlayOpen;
  return {
    ...widths,
    navigationCollapsed,
    navigationFits,
    navigationAutoHidden,
    navigationOverlay,
    navigationOverlayWidth: Math.min(
      clampWorkbenchSidebarWidth(
        AGENT_LEFT_RAIL_WIDTH,
        requested.leftRailWidth,
      ),
      viewportWidth - 16,
    ),
    leftRailBounds,
    artifactBounds,
    navigationResizable,
    compact: viewportWidth < COMPACT_BELOW,
    artifactOverlay,
    artifactOverlayOnExpand: !artifactInlineAvailable,
    artifactModal,
    artifactOverlayWidth,
    artifactOverlayLeft: viewportWidth - artifactOverlayWidth,
    chatWidth,
    gridTemplateColumns: artifactInline
      ? `${widths.leftRailWidth}px ${navigationResizable ? "8px " : ""}minmax(0, 1fr) 8px ${widths.artifactWidth}px`
      : `${widths.leftRailWidth}px ${navigationResizable ? "8px " : ""}minmax(0, 1fr)`,
  };
}
