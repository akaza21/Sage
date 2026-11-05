import { lazy, Suspense, useId, useRef, type ComponentProps } from "react";
import { capitalizeLabel } from "../../domain/text";
import { Brain, ChevronDown } from "../../ui/leanIcons";
import { flattenChoices } from "../../domain/acpSessionConfigControls";
import { ComposerMenuLoading } from "./ComposerMenuLoading";
import type ComposerEffortMenu from "./ComposerEffortMenu";

const EffortMenu = lazy(() => import("./ComposerEffortMenu"));

type Props = Omit<
  ComponentProps<typeof ComposerEffortMenu>,
  "triggerRef" | "panelId"
> & { onToggle: () => void };

export function ComposerEffortControl(props: Props) {
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelId = useId();
  const levels = flattenChoices(props.option);
  const index = levels.findIndex(
    (level) => level.value === props.option.currentValue,
  );
  if (index < 0) return null;
  const label = capitalizeLabel(levels[index].name);
  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        className="composer__control-trigger composer__effort-picker"
        data-overflow-control="effort"
        title="Select effort"
        aria-label={"Thinking level: " + label}
        aria-controls={panelId}
        aria-haspopup="dialog"
        aria-expanded={props.open}
        disabled={props.disabled}
        onPointerDown={(event) => event.currentTarget.blur()}
        onClick={props.onToggle}
      >
        <Brain className="composer__effort-icon" size={14} aria-hidden="true" />
        <span className="composer__effort-label">{label}</span>
        <ChevronDown size={12} aria-hidden="true" />
      </button>
      {props.open ? (
        <Suspense
          fallback={
            <ComposerMenuLoading
              triggerRef={props.anchorRef ?? triggerRef}
              onDismiss={props.onDismiss}
            />
          }
        >
          <EffortMenu {...props} triggerRef={triggerRef} panelId={panelId} />
        </Suspense>
      ) : null}
    </>
  );
}
