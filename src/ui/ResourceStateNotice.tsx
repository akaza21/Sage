import { CircleAlert, LoaderCircle } from "./leanIcons";

export function ResourceStateNotice({
  detail,
  state,
  title,
}: {
  detail?: string;
  state: "error" | "idle" | "loading";
  title: string;
}) {
  const error = state === "error";
  const idle = state === "idle";
  const Icon = error ? CircleAlert : LoaderCircle;

  return (
    <div
      className={`resource-state resource-state--${state}`}
      role={error ? "alert" : "status"}
      aria-atomic="true"
    >
      {idle ? null : (
        <span className="resource-state__icon" aria-hidden="true">
          <Icon size={18} strokeWidth={1.8} />
        </span>
      )}
      <strong className="resource-state__title">{title}</strong>
      {detail ? <span className="resource-state__detail">{detail}</span> : null}
    </div>
  );
}
