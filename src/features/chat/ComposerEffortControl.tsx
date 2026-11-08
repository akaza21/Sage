import { lazy, Suspense, useId, useRef, type ComponentProps } from "react";
import { capitalizeLabel } from "../../domain/text";
import { Brain, ChevronDown } from "../../ui/leanIcons";
import { selectedConfigChoice } from "../../domain/acpSessionConfigControls";
import { ComposerMenuLoading } from "./ComposerMenuLoading";
import { RESTORED_NOTE } from "./restoredSessionConfigView";
import type ComposerEffortMenu from "./ComposerEffortMenu";

const EffortMenu = lazy(() => import("./ComposerEffortMenu"));

type Props = Omit<
  ComponentProps<typeof ComposerEffortMenu>,
  "triggerRef" | "panelId"
> & { onToggle: () => void; triggerVisible?: boolean; restored?: boolean };

export function ComposerEffortControl(props: Props) {
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelId = useId();
  const choice = selectedConfigChoice(props.option);
  if (!choice) return null;
  const label = capitalizeLabel(choice.name);
  return (
    <>
      {props.triggerVisible !== false && (
        <button
          ref={triggerRef}
          type="button"
          className="composer__control-trigger composer__effort-picker"
          data-overflow-control="effort"
          title={props.restored ? RESTORED_NOTE : "Select effort"}
          aria-label={"Thinking level: " + label}
          aria-description={props.restored ? RESTORED_NOTE : undefined}
          data-restored={props.restored || undefined}
          aria-controls={panelId}
          aria-haspopup="dialog"
          aria-expanded={props.open}
          disabled={props.disabled}
          onPointerDown={(event) => event.currentTarget.blur()}
          onClick={props.onToggle}
        >
          <Brain
            className="composer__effort-icon"
            size={14}
            aria-hidden="true"
          />
          <span className="composer__effort-label">{label}</span>
          <ChevronDown size={12} aria-hidden="true" />
        </button>
      )}
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
