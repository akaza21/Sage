import { useState, type ComponentProps } from "react";
import { useWorkbenchStore } from "../../app/store";
import "../../styles/harness-picker.css";
import { PickerLogo } from "../../ui/PickerLogo";
import { SearchablePicker } from "../../ui/SearchablePicker";
import { useHarnessKeyboardFocus } from "./useHarnessKeyboardFocus";
import { LazyHarnessNotice } from "./lazyParts";
import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";
import type { HarnessNoticeDismissal } from "./harnessNoticePresentation";

type PickerProps = ComponentProps<typeof SearchablePicker>;

export function ComposerHarnessPicker({
  candidates,
  open,
  onOpenChange,
  disabled,
  selectedId,
  onSelect,
  discoveryPhase,
  canContinueInNewChat,
}: Pick<
  PickerProps,
  | "candidates"
  | "open"
  | "onOpenChange"
  | "disabled"
  | "selectedId"
  | "onSelect"
> & {
  discoveryPhase?: HarnessDiscoveryPhase;
  canContinueInNewChat?: boolean;
}) {
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const dismissal = useState<HarnessNoticeDismissal[0]>({});
  const keyboardFocus = useHarnessKeyboardFocus();
  const phase = useWorkbenchStore((state) =>
    selectedId === state.selectedHarnessId
      ? state.harnessDiscoveryPhase
      : (discoveryPhase ?? "ready"),
  );
  const harness =
    harnesses.find((harness) => harness.id === selectedId) ?? null;
  const attention =
    !harness ||
    (harness.transport === "acp" &&
      (harness.installState !== "available" ||
        !harness.bindingId ||
        (harness.characterisation ?? "characterised") !== "characterised" ||
        !!harness.update ||
        phase === "error" ||
        (harness.accountModelDiscovery &&
          phase !== "idle" &&
          phase !== "ready")));
  return (
    <label
      className="composer__control-trigger composer__harness"
      data-keyboard-focus={keyboardFocus}
    >
      <SearchablePicker
        ariaLabel="Harness"
        open={open}
        onOpenChange={onOpenChange}
        triggerIcon={<PickerLogo icon="generic" />}
        candidates={candidates}
        triggerDetail={
          attention ? (
            <span
              className="composer__harness-attention"
              aria-label="Harness status available"
            />
          ) : undefined
        }
        popoverClassName="composer__popover composer__harness-menu"
        popoverGap={8}
        popoverPlacement="above-start"
        triggerClassName="composer__control-trigger composer__picker-trigger"
        sectionLabel="Harnesses"
        searchable={false}
        popoverRole="dialog"
        disabled={disabled}
        onSelect={onSelect}
        selectedId={selectedId}
      >
        {open && (attention || canContinueInNewChat) ? (
          <div className="composer__harness-status">
            {attention ? (
              <LazyHarnessNotice
                harness={harness}
                discoveryPhase={phase}
                dismissal={dismissal}
              />
            ) : null}
            {canContinueInNewChat ? (
              <button
                type="button"
                className="button button--compact"
                onClick={() => {
                  const state = useWorkbenchStore.getState();
                  void state.openNewChatDraft(
                    state.project?.isInternal ? null : state.project?.id,
                  );
                }}
              >
                Continue in a new chat
              </button>
            ) : null}
          </div>
        ) : null}
      </SearchablePicker>
    </label>
  );
}
