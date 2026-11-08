import type { ComponentProps } from "react";
import { useWorkbenchStore } from "../../app/store";
import "../../styles/harness-picker.css";
import { Settings } from "../../ui/leanIcons";
import { HarnessSignInPicker } from "../harness/HarnessSignInPicker";
import { useHarnessKeyboardFocus } from "./useHarnessKeyboardFocus";

type PickerProps = ComponentProps<typeof HarnessSignInPicker>;

/** The composer's harness switcher, with detection progress. */
export function ComposerHarnessPicker({
  candidates,
  open,
  onOpenChange,
  disabled,
  selectedId,
  onSelect,
}: Pick<
  PickerProps,
  | "candidates"
  | "open"
  | "onOpenChange"
  | "disabled"
  | "selectedId"
  | "onSelect"
>) {
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const keyboardFocus = useHarnessKeyboardFocus();
  const pending = harnesses.some(
    (harness) => harness.installState === "detecting",
  );
  return (
    <label
      className="composer__control-trigger composer__harness"
      data-keyboard-focus={keyboardFocus}
    >
      <HarnessSignInPicker
        harnesses={harnesses}
        ariaLabel="Harness"
        open={open}
        onOpenChange={onOpenChange}
        triggerIcon={<Settings size={16} aria-hidden="true" />}
        candidates={candidates}
        popoverClassName="composer__popover composer__harness-menu"
        popoverGap={8}
        popoverPlacement="above-start"
        triggerClassName="composer__control-trigger composer__picker-trigger"
        sectionLabel="Harnesses"
        searchable={false}
        disabled={disabled}
        state={pending ? "loading" : undefined}
        stateTitle={pending ? "Detecting harnesses…" : undefined}
        onSelect={onSelect}
        selectedId={selectedId}
      />
    </label>
  );
}
