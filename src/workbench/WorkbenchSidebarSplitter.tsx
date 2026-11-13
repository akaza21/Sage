import { Splitter } from "../ui/Splitter";
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
  return (
    <Splitter
      ariaLabel={ariaLabel}
      className={`workbench-sidebar-splitter workbench-sidebar-splitter--${side}`}
      activeClassName="workbench-sidebar-splitter--active"
      value={value}
      min={width.minWidth}
      max={width.maxWidth}
      onPointerResize={(clientX, splitter) => {
        const bounds = splitter.parentElement?.getBoundingClientRect();
        if (!bounds) return;
        const pointerOffset =
          side === "leading"
            ? clientX - bounds.left - pointerOffsetAdjustment
            : bounds.right - clientX;
        onChange(resizeWorkbenchSidebar(width, { pointerOffset }));
      }}
      onKeyResize={(key, shiftKey) => {
        onChange(
          resizeWorkbenchSidebar(width, {
            currentWidth: value,
            // A trailing sidebar grows towards the left.
            key:
              side === "leading"
                ? key
                : key === "ArrowLeft"
                  ? "ArrowRight"
                  : key === "ArrowRight"
                    ? "ArrowLeft"
                    : key,
            shiftKey,
          }),
        );
      }}
    />
  );
}
