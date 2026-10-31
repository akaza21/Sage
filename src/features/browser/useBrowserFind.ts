import { useRef, useState, type RefObject } from "react";

/** Menu and native shortcuts share the same focus and dismissal lifecycle. */
export function useBrowserFind(
  surface: RefObject<HTMLElement | null>,
  address: RefObject<HTMLInputElement | null>,
  stop: () => void,
) {
  const [findOpen, setFindOpen] = useState(false);
  const findField = useRef<HTMLInputElement>(null);
  const launcher = useRef<HTMLElement | null>(null);
  return {
    findOpen,
    findField,
    openFind: () => {
      if (!findOpen) {
        const active = document.activeElement;
        launcher.current =
          active instanceof HTMLElement && surface.current?.contains(active)
            ? active
            : address.current;
        setFindOpen(true);
      } else {
        findField.current?.focus();
        findField.current?.select();
      }
    },
    closeFind: () => {
      setFindOpen(false);
      if (launcher.current?.isConnected) launcher.current.focus();
      else address.current?.focus();
      stop();
    },
  };
}
