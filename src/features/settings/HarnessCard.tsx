import { useId } from "react";
import type { HarnessDefinition } from "../../domain/harness";
import HarnessAccountDetails from "./HarnessAccountDetails";
import { HARNESS_BADGE_LABEL, type HarnessCardStatus } from "./harnessCardStatus";

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
  return (
    <section
      className="harness-card"
      data-status={status.badge}
      data-selected={selected || undefined}
      aria-labelledby={headingId}
    >
      <div className="harness-card__header">
        <span className="harness-card__mark" aria-hidden="true">
          {harness.name.charAt(0).toUpperCase()}
        </span>
        <div className="harness-card__identity">
          <div className="harness-card__title">
            <h3 id={headingId}>{harness.name}</h3>
            {harness.version ? (
              <code className="harness-card__version">
                {/^\d/.test(harness.version)
                  ? `v${harness.version}`
                  : harness.version}
              </code>
            ) : null}
            <span className="harness-badge" data-status={status.badge}>
              <span className="harness-badge__dot" aria-hidden="true" />
              {HARNESS_BADGE_LABEL[status.badge]}
            </span>
          </div>
          <p className="harness-card__summary">
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
      {harness.version || runnable ? (
        <details
          className="harness-card__details"
          open={(selected && runnable) || status.badge === "signInNeeded"}
        >
          <summary>Details</summary>
          <div className="harness-card__body">
            <HarnessAccountDetails
              harness={harness}
              selected={selected}
              available={runnable}
              showSignInHint={status.showSignInHint}
              disabled={detailsDisabled}
            />
          </div>
        </details>
      ) : null}
    </section>
  );
}
