import { lazy, Suspense, useId, useRef, type ComponentProps } from "react";
import { capitalizeLabel } from "../../domain/text";
import { Brain, ChevronDown } from "../../ui/leanIcons";
import { selectedConfigChoice } from "../../domain/acpSessionConfigControls";
import { ComposerMenuLoading } from "./ComposerMenuLoading";
import type ComposerEffortMenu from "./ComposerEffortMenu";

const EffortMenu = lazy(() => import("./ComposerEffortMenu"));

type Props = Omit<
  ComponentProps<typeof ComposerEffortMenu>,
  "triggerRef" | "panelId"
> & { onToggle: () => void; triggerVisible?: boolean; restored?: string };

/** The Thinking level trigger's icon, label and chevron, the same whether
 * the control is open to choose or disabled until levels are known. */
export function EffortTriggerFace({ label }: { label: string }) {
  return (
    <>
      <Brain className="composer__effort-icon" size={14} aria-hidden="true" />
      <span className="composer__effort-label">{label}</span>
      <ChevronDown
        className="composer__effort-chevron"
        size={12}
        aria-hidden="true"
      />
    </>
  );
}

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
          title={props.restored ?? "Select effort"}
          aria-label={"Thinking level: " + label}
          aria-description={props.restored}
          data-restored={props.restored ? true : undefined}
          aria-controls={panelId}
          aria-haspopup="dialog"
          aria-expanded={props.open}
          disabled={props.disabled}
          onPointerDown={(event) => event.currentTarget.blur()}
          onClick={props.onToggle}
        >
          <EffortTriggerFace label={label} />
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
