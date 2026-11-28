import { useCallback, useEffect, useMemo, useState } from "react";

import {
  AGENT_ICON_RAIL_WIDTH,
  type AgentDesktopSidebarPriority,
  type AgentDesktopSidebarWidths,
} from "./workbenchSidebarResize";
import { agentWorkbenchLayout } from "./agentWorkbenchLayout";
import { writeWorkbenchLayout } from "./workbenchLayoutStorage";

export function useAgentDesktopSidebarWidths(
  artifactExpanded: boolean,
  navigationCollapsed = false,
  navigationOverlayOpen = false,
  storedSidebarWidth?: number,
) {
  const [viewportWidth, setViewportWidth] = useState(window.innerWidth);
  const [widthRequests, setWidthRequests] = useState<
    Partial<AgentDesktopSidebarWidths>
  >(() =>
    storedSidebarWidth === undefined
      ? {}
      : { leftRailWidth: storedSidebarWidth + AGENT_ICON_RAIL_WIDTH },
  );
  const [priority, setPriority] =
    useState<AgentDesktopSidebarPriority>("artifact");

  useEffect(() => {
    const resize = () => setViewportWidth(window.innerWidth);
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, []);

  const changeWidth = useCallback(
    (prioritize: AgentDesktopSidebarPriority, width: number) => {
      setPriority(prioritize);
      if (prioritize === "left")
        writeWorkbenchLayout({
          agentSidebarWidth: width - AGENT_ICON_RAIL_WIDTH,
        });
      setWidthRequests((requests) => ({
        ...requests,
        [prioritize === "left" ? "leftRailWidth" : "artifactWidth"]: width,
      }));
    },
    [],
  );

  const layout = useMemo(
    () =>
      agentWorkbenchLayout({
        viewportWidth,
        artifactExpanded,
        navigationCollapsed,
        navigationOverlayOpen,
        requestedWidths: widthRequests,
        prioritize: priority,
      }),
    [
      artifactExpanded,
      navigationCollapsed,
      navigationOverlayOpen,
      priority,
      viewportWidth,
      widthRequests,
    ],
  );

  return { changeWidth, layout };
}
