import { useState, type KeyboardEvent, type PointerEvent } from "react";

export type SplitterKey = "ArrowLeft" | "ArrowRight" | "Home" | "End";
const SPLITTER_KEYS = ["ArrowLeft", "ArrowRight", "Home", "End"];

export function Splitter({
  ariaLabel,
  className,
  activeClassName,
  value,
  min,
  max,
  onPointerResize,
  onKeyResize,
}: {
  ariaLabel: string;
  className: string;
  activeClassName: string;
  value: number;
  min: number;
  max: number;
  onPointerResize: (clientX: number, splitter: HTMLDivElement) => void;
  onKeyResize: (key: SplitterKey, large: boolean) => void;
}) {
  const [resizing, setResizing] = useState(false);
  const endResize = () => setResizing(false);
  return (
    <div
      className={resizing ? `${className} ${activeClassName}` : className}
      role="separator"
      aria-label={ariaLabel}
      aria-orientation="vertical"
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={value}
      tabIndex={0}
      onKeyDown={(event: KeyboardEvent<HTMLDivElement>) => {
        if (
          !SPLITTER_KEYS.includes(event.key) ||
          event.altKey ||
          event.ctrlKey ||
          event.metaKey
        )
          return;
        event.preventDefault();
        onKeyResize(event.key as SplitterKey, event.shiftKey);
      }}
      onPointerDown={(event: PointerEvent<HTMLDivElement>) => {
        event.currentTarget.setPointerCapture?.(event.pointerId);
        setResizing(true);
        onPointerResize(event.clientX, event.currentTarget);
      }}
      onPointerMove={(event) => {
        if (resizing) onPointerResize(event.clientX, event.currentTarget);
      }}
      // The browser releases the capture itself after pointerup.
      onPointerUp={endResize}
      onPointerCancel={endResize}
      onLostPointerCapture={endResize}
    />
  );
}
