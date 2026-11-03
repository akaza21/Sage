import { useCallback, useState } from "react";

/** A callback-ref element that survives Activity hiding, which detaches refs but keeps the DOM node connected. */
export function useRetainedElement<T extends Element>() {
  const [element, setElement] = useState<T | null>(null);
  const ref = useCallback((next: T | null) => {
    if (next) {
      setElement(next);
      return;
    }
    queueMicrotask(() =>
      setElement((current) => (current?.isConnected ? current : null)),
    );
  }, []);
  return [element, ref] as const;
}
