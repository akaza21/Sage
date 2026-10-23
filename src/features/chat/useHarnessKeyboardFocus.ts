import { useEffect, useState } from "react";

export function useHarnessKeyboardFocus() {
  const [keyboard, setKeyboard] = useState(false);
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!event.altKey && !event.ctrlKey && !event.metaKey) setKeyboard(true);
    };
    const onPointerDown = () => setKeyboard(false);
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("pointerdown", onPointerDown, true);
    };
  }, []);
  return keyboard;
}
