import { useEffect, useState } from "react";

export function useHarnessKeyboardFocus() {
  const [keyboard, setKeyboard] = useState(false);
  useEffect(() => {
    const events = ["keydown", "pointerdown"] as const;
    const onInput = (event: KeyboardEvent | PointerEvent) => {
      if (
        event.type === "pointerdown" ||
        (!event.altKey && !event.ctrlKey && !event.metaKey)
      )
        setKeyboard(event.type === "keydown");
    };
    events.forEach((type) => window.addEventListener(type, onInput, true));
    return () =>
      events.forEach((type) => window.removeEventListener(type, onInput, true));
  }, []);
  return keyboard;
}
