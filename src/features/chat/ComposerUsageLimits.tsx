import { useEffect, useState, type CSSProperties, type RefObject } from "react";
import type {
  AcpUsageRequest,
  AcpUsageState,
  UsageWindow,
} from "../../domain/acpUsage";
import { errorMessage } from "../../domain/errors";
import { acpUsageDesktop } from "../../services/acpUsageDesktop";
import { MenuPopover } from "../../ui/MenuPopover";
import "./composer-usage-limits.css";

const NOTICE: Record<Exclude<AcpUsageState["kind"], "usage">, string> = {
  unsupported: "This account does not report usage limits.",
  offline: "Usage limits are available while this chat's session is running.",
  unavailable: "The harness has not reported usage limits yet.",
};

function windowSpan(minutes: number | null): string | null {
  if (!minutes) return null;
  if (minutes === 10_080) return "Weekly";
  const [size, unit] =
    minutes % 1_440 === 0
      ? [minutes / 1_440, "day"]
      : minutes % 60 === 0
        ? [minutes / 60, "hour"]
        : [minutes, "minute"];
  return `${size}-${unit}`;
}

function resetText(seconds: number): string {
  const at = new Date(seconds * 1_000);
  const sameDay = at.toDateString() === new Date().toDateString();
  return `Resets ${at.toLocaleString(undefined, {
    ...(sameDay ? {} : { weekday: "short" }),
    hour: "numeric",
    minute: "2-digit",
  })}`;
}

function UsageBar({ window }: { window: UsageWindow }) {
  const title =
    [window.label, windowSpan(window.windowMinutes)]
      .filter(Boolean)
      .join(" · ") || "Usage";
  return (
    <li className="composer-usage__window">
      <span className="composer-usage__row">
        <span>{title}</span>
        <span>{window.usedPercent}%</span>
      </span>
      <span
        className="composer-usage__bar"
        role="progressbar"
        aria-label={`${title} used`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={window.usedPercent}
        style={{ "--usage": `${window.usedPercent}%` } as CSSProperties}
      />
      {window.resetsAt !== null ? (
        <span className="composer-usage__reset">
          {resetText(window.resetsAt)}
        </span>
      ) : null}
    </li>
  );
}

export default function ComposerUsageLimits({
  id,
  request,
  triggerRef,
  onDismiss,
}: {
  id: string;
  request: AcpUsageRequest;
  triggerRef: RefObject<HTMLElement | null>;
  onDismiss: () => void;
}) {
  const [state, setState] = useState<AcpUsageState | string | null>(null);
  useEffect(() => {
    let current = true;
    acpUsageDesktop.read(request).then(
      (next) => current && setState(next),
      (error: unknown) => current && setState(errorMessage(error)),
    );
    return () => {
      current = false;
    };
  }, [request]);
  return (
    <MenuPopover
      id={id}
      ariaLabel="Usage limits"
      role="dialog"
      focusStrategy="none"
      placement="above-end"
      open
      triggerRef={triggerRef}
      onDismiss={onDismiss}
      className="composer__popover composer-usage"
    >
      <div className="composer-usage__heading">
        <strong>Usage limits</strong>
        {typeof state === "object" &&
        state?.kind === "usage" &&
        state.snapshot.plan ? (
          <span>{state.snapshot.plan}</span>
        ) : null}
      </div>
      {state === null ? (
        <p role="status">Checking usage…</p>
      ) : typeof state === "string" ? (
        <p role="alert">{state}</p>
      ) : state.kind !== "usage" ? (
        <p role="status">{NOTICE[state.kind]}</p>
      ) : (
        <>
          {state.snapshot.limitReached ? (
            <p role="alert" className="composer-usage__reached">
              Usage limit reached.
            </p>
          ) : null}
          <ul className="composer-usage__windows">
            {state.snapshot.windows.map((window, index) => (
              <UsageBar key={index} window={window} />
            ))}
          </ul>
        </>
      )}
    </MenuPopover>
  );
}
