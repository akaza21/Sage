import { useCallback, useEffect, useRef, useState } from "react";

import {
  type AgentDesktopSidebarPriority,
  type AgentDesktopSidebarWidths,
  fitAgentDesktopSidebarWidths,
  initialAgentDesktopSidebarWidths,
} from "./workbenchSidebarResize";

type AgentSidebarOverrides = {
  left: boolean;
  artifact: boolean;
  prioritize: AgentDesktopSidebarPriority;
};

export function useAgentDesktopSidebarWidths() {
  const initialWidths = initialAgentDesktopSidebarWidths(window.innerWidth);
  const requestedWidths = useRef<AgentDesktopSidebarWidths>(initialWidths);
  const overrides = useRef<AgentSidebarOverrides>({
    left: false,
    artifact: false,
    prioritize: "artifact",
  });
  const [renderedWidths, setRenderedWidths] = useState(initialWidths);
  const fitRequestedWidths = useCallback(() => {
    const defaults = initialAgentDesktopSidebarWidths(window.innerWidth);
    return fitAgentDesktopSidebarWidths({
      viewportWidth: window.innerWidth,
      leftRailWidth: overrides.current.left
        ? requestedWidths.current.leftRailWidth
        : defaults.leftRailWidth,
      artifactWidth: overrides.current.artifact
        ? requestedWidths.current.artifactWidth
        : defaults.artifactWidth,
      prioritize: overrides.current.prioritize,
    });
  }, []);

  useEffect(() => {
    const resize = () => setRenderedWidths(fitRequestedWidths());
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, [fitRequestedWidths]);

  const changeWidth = useCallback(
    (prioritize: AgentDesktopSidebarPriority, width: number) => {
      overrides.current = {
        ...overrides.current,
        [prioritize]: true,
        prioritize,
      };
      requestedWidths.current = {
        ...requestedWidths.current,
        ...(prioritize === "left"
          ? { leftRailWidth: width }
          : { artifactWidth: width }),
      };
      setRenderedWidths(fitRequestedWidths());
    },
    [fitRequestedWidths],
  );

  return { changeWidth, renderedWidths };
}
