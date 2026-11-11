import {
  useEffect,
  useId,
  useState,
  type CSSProperties,
  type RefObject,
} from "react";
import type {
  AcpUsageRequest,
  AcpUsageState,
  SessionSpend,
  UsageWindow,
} from "../../domain/acpUsage";
import { errorMessage } from "../../domain/errors";
import { acpUsageDesktop } from "../../services/acpUsageDesktop";
import { MenuPopover } from "../../ui/MenuPopover";
import "./composer-usage-limits.css";

const NOTICE: Record<Exclude<AcpUsageState["kind"], "usage">, string> = {
  unsupported: "This account does not report usage.",
  offline: "Usage is available while this chat's session is running.",
  unavailable: "The harness has not reported usage yet.",
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

function costText(usd: number): string {
  // Sub-dollar sessions keep their cents' fractions, as harnesses price them.
  return usd.toLocaleString(undefined, {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: 2,
    maximumFractionDigits: usd < 1 ? 4 : 2,
  });
}

function SessionSpendRows({
  spend,
  labelledBy,
}: {
  spend: SessionSpend;
  labelledBy: string;
}) {
  const count = (tokens: number) => tokens.toLocaleString();
  // Cache rows show only when the harness used its cache.
  const rows = (
    [
      ["Input", count(spend.inputTokens)],
      ["Output", count(spend.outputTokens)],
      [
        "Cache read",
        spend.cacheReadTokens ? count(spend.cacheReadTokens) : null,
      ],
      [
        "Cache write",
        spend.cacheWriteTokens ? count(spend.cacheWriteTokens) : null,
      ],
      ["Total tokens", count(spend.totalTokens)],
      ["Cost", spend.costUsd === null ? null : costText(spend.costUsd)],
    ] satisfies [string, string | null][]
  ).filter((row): row is [string, string] => row[1] !== null);
  return (
    <dl className="composer-usage__session" aria-labelledby={labelledBy}>
      {rows.map(([term, value]) => (
        <div key={term} className="composer-usage__row">
          <dt>{term}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
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
  const headingId = useId();
  const sessionHeadingId = useId();
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
  const reported = typeof state === "object" ? state : null;
  const snapshot = reported?.kind === "usage" ? reported.snapshot : null;
  // A harness that reports only its session's spend has no limits to name.
  const sessionOnly = Boolean(
    snapshot && snapshot.windows.length === 0 && snapshot.session,
  );
  return (
    <MenuPopover
      id={id}
      ariaLabel="Usage"
      role="dialog"
      focusStrategy="none"
      placement="above-end"
      open
      triggerRef={triggerRef}
      onDismiss={onDismiss}
      className="composer__popover composer-usage"
    >
      <div className="composer-usage__heading">
        <strong id={headingId}>
          {sessionOnly ? "This session" : "Usage limits"}
        </strong>
        {snapshot?.plan ? <span>{snapshot.plan}</span> : null}
      </div>
      {state === null ? (
        <p role="status">Checking usage…</p>
      ) : typeof state === "string" ? (
        <p role="alert">{state}</p>
      ) : state.kind !== "usage" ? (
        <p role="status">{NOTICE[state.kind]}</p>
      ) : snapshot ? (
        <>
          {snapshot.limitReached ? (
            <p role="alert" className="composer-usage__reached">
              Usage limit reached.
            </p>
          ) : null}
          {snapshot.windows.length > 0 ? (
            <ul className="composer-usage__windows">
              {snapshot.windows.map((window, index) => (
                <UsageBar key={index} window={window} />
              ))}
            </ul>
          ) : null}
          {snapshot.session ? (
            <>
              {sessionOnly ? null : (
                <strong
                  id={sessionHeadingId}
                  className="composer-usage__subheading"
                >
                  This session
                </strong>
              )}
              <SessionSpendRows
                spend={snapshot.session}
                labelledBy={sessionOnly ? headingId : sessionHeadingId}
              />
            </>
          ) : null}
        </>
      ) : null}
    </MenuPopover>
  );
}
