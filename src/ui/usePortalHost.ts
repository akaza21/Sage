import {
  useLayoutEffect,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";

/** Adopt one stable portal host without remounting the resource it contains. */
export function usePortalHost(
  container: HTMLElement | null | RefObject<HTMLElement | null>,
  className = "",
) {
  const [host] = useState(() => {
    const element = document.createElement("div");
    element.className = className;
    return element;
  });
  useLayoutEffect(() => {
    const target =
      container && "current" in container ? container.current : container;
    target?.append(host);
    return () => host.remove();
  }, [container, host]);
  return {
    host,
    portal: (children: ReactNode) => createPortal(children, host),
  };
}
