import { useId } from "react";
import { LoaderCircle } from "./leanIcons";
import { Notice } from "./Notice";

export function ResourceStateNotice({
  detail,
  state,
  title,
  identity,
}: {
  detail?: string;
  state: "error" | "idle" | "loading";
  title: string;
  /** Resource/problem identity; legacy callers are scoped to this mounted view. */
  identity?: string;
}) {
  const scope = useId();
  if (state === "error") {
    return (
      <Notice
        identity={identity ?? `resource-load:${scope}`}
        severity="blocking"
        message={title}
        details={detail}
      />
    );
  }
  const idle = state === "idle";

  return (
    <div
      className={`resource-state resource-state--${state}`}
      role="status"
      aria-atomic="true"
    >
      {idle ? null : (
        <span className="resource-state__icon" aria-hidden="true">
          <LoaderCircle size={18} strokeWidth={1.8} />
        </span>
      )}
      <strong className="resource-state__title">{title}</strong>
      {detail ? <span className="resource-state__detail">{detail}</span> : null}
    </div>
  );
}
