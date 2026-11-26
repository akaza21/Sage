import { useId } from "react";
import type { HarnessDefinition } from "../../domain/harness";
import { PickerLogo } from "../../ui/PickerLogo";
import HarnessAccountDetails from "./HarnessAccountDetails";
import {
  HARNESS_BADGE_LABEL,
  type HarnessCardStatus,
} from "./harnessCardStatus";

/** One harness: a header row to scan, and its details below. */
export function HarnessCard({
  harness,
  status,
  note,
  selected,
  actionDisabled,
  detailsDisabled,
  onSelect,
}: {
  harness: HarnessDefinition;
  status: HarnessCardStatus;
  /** A qualifier on the version, such as an untested release. */
  note: string | null;
  selected: boolean;
  actionDisabled: boolean;
  detailsDisabled: boolean;
  onSelect: () => void;
}) {
  const headingId = useId();
  const runnable =
    status.badge !== "unavailable" && status.badge !== "notInstalled";
  const { version, update } = harness;
  return (
    <section
      className="agent-settings__card harness-card"
      data-status={status.badge}
      data-selected={selected || undefined}
      aria-labelledby={headingId}
    >
      <div className="agent-settings__row">
        {/* Curated brand metadata picks the mark; others get the generic one. */}
        <PickerLogo icon={harness.brand ?? "generic"} />
        <div className="agent-settings__copy harness-card__copy">
          <div className="harness-card__title">
            <h3 id={headingId}>{harness.name}</h3>
            {version ? (
              <code>{/^\d/.test(version) ? `v${version}` : version}</code>
            ) : null}
            <span className="harness-badge" data-status={status.badge}>
              {HARNESS_BADGE_LABEL[status.badge]}
            </span>
            {update ? (
              <span className="harness-badge">
                Update {update.latestVersion}
              </span>
            ) : null}
          </div>
          <p>
            {note ? `${note}. ` : ""}
            {status.summary}
          </p>
        </div>
        {runnable ? (
          <button
            className="button button--compact"
            type="button"
            disabled={actionDisabled}
            onClick={onSelect}
          >
            {selected ? "Check models and sign-in" : "Select and check"}
          </button>
        ) : null}
      </div>
      {version || runnable ? (
        <details
          open={(selected && runnable) || status.badge === "signInNeeded"}
        >
          <summary>Details</summary>
          <HarnessAccountDetails
            harness={harness}
            selected={selected}
            available={runnable}
            showSignInHint={status.showSignInHint}
            disabled={detailsDisabled}
          />
        </details>
      ) : null}
    </section>
  );
}
