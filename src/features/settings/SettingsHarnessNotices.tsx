import { harnessUnavailableReason } from "../../domain/harness";
import { Info, X } from "../../ui/leanIcons";
import type { HarnessNoticePresentationProps } from "../chat/harnessNoticePresentation";

/** Preserve Settings output while chat notices migrate independently. */
export function SettingsHarnessNotices({
  harness,
  binding,
  builds = [],
  discovery,
  actions,
}: HarnessNoticePresentationProps) {
  const update = harness?.update;
  const howToUpdate = update ? (
    <>
      Run <code>{update.command}</code>, then refresh discovery in Settings.
    </>
  ) : (
    "Update it, then refresh harness discovery in Settings."
  );
  if (binding)
    return (
      <div className="composer__notice" role="status">
        {harness?.unavailableReason === "versionTooOld" ? (
          <>
            {binding.message} {howToUpdate}
          </>
        ) : (
          harnessUnavailableReason(
            harness?.unavailableReason,
            harness?.name ?? "This harness",
          )
        )}
      </div>
    );
  return (
    <>
      {builds.length ? (
        <div className="build-notice" role="status">
          <Info aria-hidden="true" />
          <span>
            {builds.map((build) => (
              <span key={build.identity}>
                {build.severity === "warning" &&
                harness?.characterisation === "newerThanTested"
                  ? `${harness.name} ${harness.boundVersion} is newer than Sage has tested. Tested features stay on.`
                  : build.message}{" "}
                {build.details ? howToUpdate : null}
              </span>
            ))}
          </span>
          <button
            type="button"
            aria-label="Dismiss build notice"
            title="Dismiss"
            onClick={() => builds.forEach((build) => build.action?.onClick())}
          >
            <X aria-hidden="true" />
          </button>
        </div>
      ) : null}
      {discovery ? (
        <div
          className={
            discovery.severity === "blocking"
              ? "composer__error"
              : "composer__notice"
          }
          role={discovery.severity === "blocking" ? "alert" : "status"}
        >
          {discovery.message}
          {actions}
        </div>
      ) : null}
    </>
  );
}
