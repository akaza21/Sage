import { lazy, Suspense, type ComponentProps } from "react";
import { capitalizeLabel } from "../../domain/text";
import { selectedConfigChoice } from "../../domain/acpSessionConfigControls";
import type ComposerEffortMenu from "./ComposerEffortMenu";

const EffortMenu = lazy(() => import("./ComposerEffortMenu"));

/**
 * A thinking-level option shows as the level beside the model name, and as a
 * section of the model popover where it is changed.
 */
export function ComposerEffortControl({
  mode,
  ...props
}: ComponentProps<typeof ComposerEffortMenu> & { mode: "label" | "panel" }) {
  const choice = selectedConfigChoice(props.option);
  if (!choice) return null;
  return mode === "label" ? (
    <span className="composer__model-effort">
      {capitalizeLabel(choice.name)}
    </span>
  ) : (
    <Suspense fallback={null}>
      <EffortMenu {...props} />
    </Suspense>
  );
}
