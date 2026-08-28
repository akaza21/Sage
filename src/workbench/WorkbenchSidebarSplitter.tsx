import { useRef, useState, type KeyboardEvent, type PointerEvent } from "react";

import {
  type WorkbenchSidebarWidth,
  resizeWorkbenchSidebar,
} from "./workbenchSidebarResize";

export function WorkbenchSidebarSplitter({
  ariaLabel,
  side,
  value,
  width,
  pointerOffsetAdjustment = 0,
  onChange,
}: {
  ariaLabel: string;
  side: "leading" | "trailing";
  value: number;
  width: WorkbenchSidebarWidth;
  pointerOffsetAdjustment?: number;
  onChange: (next: number) => void;
}) {
  const [resizing, setResizing] = useState(false);
  const splitterRef = useRef<HTMLDivElement>(null);

  function resizeFromPointer(event: PointerEvent<HTMLDivElement>) {
    const bounds = splitterRef.current?.parentElement?.getBoundingClientRect();
    if (!bounds) return;
    const pointerOffset =
      side === "leading"
        ? event.clientX - bounds.left - pointerOffsetAdjustment
        : bounds.right - event.clientX;
    onChange(resizeWorkbenchSidebar(width, { pointerOffset }));
  }

  function beginResize(event: PointerEvent<HTMLDivElement>) {
    event.currentTarget.setPointerCapture?.(event.pointerId);
    setResizing(true);
    resizeFromPointer(event);
  }

  function endResize(event: PointerEvent<HTMLDivElement>) {
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) {
      event.currentTarget.releasePointerCapture?.(event.pointerId);
    }
    setResizing(false);
  }

  function resizeByKeyboard(event: KeyboardEvent<HTMLDivElement>) {
    const key =
      side === "leading"
        ? event.key
        : event.key === "ArrowLeft"
          ? "ArrowRight"
          : event.key === "ArrowRight"
            ? "ArrowLeft"
            : event.key;
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(key)) return;
    event.preventDefault();
    onChange(
      resizeWorkbenchSidebar(width, {
        currentWidth: value,
        key,
        shiftKey: event.shiftKey,
      }),
    );
  }

  return (
    <div
      ref={splitterRef}
      className={`workbench-sidebar-splitter${
        resizing ? " workbench-sidebar-splitter--active" : ""
      }`}
      role="separator"
      aria-label={ariaLabel}
      aria-orientation="vertical"
      aria-valuemin={width.minWidth}
      aria-valuemax={width.maxWidth}
      aria-valuenow={value}
      tabIndex={0}
      onKeyDown={resizeByKeyboard}
      onPointerDown={beginResize}
      onPointerMove={(event) => {
        if (resizing) resizeFromPointer(event);
      }}
      onPointerUp={endResize}
      onPointerCancel={endResize}
    />
  );
}
