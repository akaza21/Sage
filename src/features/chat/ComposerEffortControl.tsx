import { lazy, Suspense, useLayoutEffect, type ComponentProps } from "react";
import { capitalizeLabel } from "../../domain/text";
import { selectedConfigChoice } from "../../domain/acpSessionConfigControls";
import type ComposerEffortMenu from "./ComposerEffortMenu";

const EffortMenu = lazy(() => import("./ComposerEffortMenu"));

/**
 * A thinking-level option shows as the level beside the model name, and as a
 * section of the model popover where it is changed. The label reports the
 * level it shows so the model chip can describe it.
 */
export function ComposerEffortControl({
  mode,
  onLabel,
  ...props
}: ComponentProps<typeof ComposerEffortMenu> & {
  mode: "label" | "panel";
  onLabel?: (label: string | null) => void;
}) {
  const choice = selectedConfigChoice(props.option);
  const label = choice ? capitalizeLabel(choice.name) : null;
  useLayoutEffect(() => {
    if (mode === "label") onLabel?.(label);
  }, [label, mode, onLabel]);
  if (!label) return null;
  return mode === "label" ? (
    <span className="composer__model-effort">{label}</span>
  ) : (
    <Suspense fallback={null}>
      <EffortMenu {...props} />
    </Suspense>
  );
}
