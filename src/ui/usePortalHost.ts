import { useLayoutEffect, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

/** Adopt one stable portal host without remounting the resource it contains. */
export function usePortalHost(container: HTMLElement | null, className = "") {
  const [host] = useState(() => {
    const element = document.createElement("div");
    element.className = className;
    return element;
  });
  useLayoutEffect(() => {
    container?.append(host);
    return () => host.remove();
  }, [container, host]);
  return {
    host,
    portal: (children: ReactNode) => createPortal(children, host),
  };
}
