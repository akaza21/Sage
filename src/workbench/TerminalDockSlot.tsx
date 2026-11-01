import { useCallback, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

/**
 * The terminal dock is the center region's bottom panel in both window
 * modes, but its sessions must outlive a mode switch. The dock renders once
 * into a detached host element through a portal at a stable tree position;
 * whichever center region is mounted adopts that host into its slot. Moving
 * the host keeps the dock mounted, so no session is closed and reopened.
 */
export function useTerminalDockHost() {
  const [host] = useState(() => {
    const element = document.createElement("div");
    element.className = "terminal-dock-host";
    return element;
  });
  const slot = useCallback(
    (container: HTMLDivElement | null) => container?.append(host),
    [host],
  );
  return {
    portal: (dock: ReactNode) => createPortal(dock, host),
    slot: <div className="center-region__dock" ref={slot} />,
  };
}
