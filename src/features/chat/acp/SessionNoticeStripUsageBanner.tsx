// Named under SessionNoticeStrip so the bundler keeps it in that lazy chunk,
// not the Agent workbench chunk (see vite.config.ts).
import { useState } from "react";

import type { AcpChatPage, SessionNotice } from "../../../domain/acpProjection";
import { CircleAlert, TriangleAlert } from "../../../ui/leanIcons";
import "./usage-limit-banner.css";

/** One window of the harness's account, as its binding reported it. */
interface UsageLimit {
  provider: string;
  label: string | null;
  usedPercent: number | null;
  windowMinutes: number | null;
  /** Unix seconds. */
  resetsAt: number | null;
}

function count(value: unknown, max: number): number | null {
  const n = Number.isSafeInteger(value) ? (value as number) : -1;
  return n >= 0 && n <= max ? n : null;
}
const label = (value: unknown) =>
  typeof value === "string" && value && value.length <= 128 ? value : null;

/** The notice's usage limit, or `null` when it reports none. */
function usageLimitOf(notice: SessionNotice): UsageLimit | null {
  const record = notice.usageLimit;
  if (!record || typeof record !== "object") return null;
  const value = record as Record<string, unknown>;
  const provider = label(value.provider);
  return provider
    ? {
        provider,
        label: label(value.label),
        usedPercent: count(value.usedPercent, 100),
        windowMinutes: count(value.windowMinutes, 2 ** 32),
        resetsAt: count(value.resetsAt, 253_402_300_799),
      }
    : null;
}

function span(minutes: number | null): string {
  if (!minutes) return "";
  if (minutes === 10_080) return "weekly ";
  const [size, unit] =
    minutes % 1_440 === 0
      ? [minutes / 1_440, "day"]
      : minutes % 60 === 0
        ? [minutes / 60, "hour"]
        : [minutes, "minute"];
  return `${size}-${unit} `;
}

function resets(seconds: number): string {
  const at = new Date(seconds * 1_000);
  const sameDay = at.toDateString() === new Date().toDateString();
  return ` · resets ${at.toLocaleString(undefined, {
    ...(sameDay ? {} : { weekday: "short" }),
    hour: "numeric",
    minute: "2-digit",
  })}`;
}

function usageLimitText(reached: boolean, limit: UsageLimit): string {
  const name = `${span(limit.windowMinutes)}${limit.provider}${
    limit.label ? ` ${limit.label}` : ""
  } limit`;
  const used = limit.usedPercent;
  const lead = reached
    ? `You've reached your ${name}`
    : used !== null
      ? `${used}% of your ${name} used`
      : `You're close to your ${name}`;
  return lead + (limit.resetsAt !== null ? resets(limit.resetsAt) : "");
}

const versionOf = (notice: SessionNotice) =>
  JSON.stringify([notice.identity, notice.level]);

interface Dismissed {
  session: string;
  versions: readonly string[];
}

/** The harness's usage-limit warnings, just above the composer: one per
 * window, each hidden once dismissed until its level changes, and once its
 * window has reset. */
export function UsageLimitBanner({ page }: { page: AcpChatPage }) {
  const session = JSON.stringify([page.chatId, page.currentSessionId]);
  // A window that reset before this mount is stale; one resetting later
  // clears with the harness's next report.
  const [now] = useState(() => Date.now() / 1_000);
  const limits = (page.sessionNotices ?? []).flatMap((notice) => {
    const limit = notice.level === "info" ? null : usageLimitOf(notice);
    return limit && !(limit.resetsAt !== null && limit.resetsAt <= now)
      ? [{ notice, limit }]
      : [];
  });
  const [dismissed, setDismissed] = useState<Dismissed>(() => ({
    session,
    versions: [],
  }));
  const current = limits.map(({ notice }) => versionOf(notice));
  // Dismissals belong to one session and to the limits it still reports.
  const kept =
    dismissed.session === session
      ? dismissed.versions.filter((version) => current.includes(version))
      : [];
  if (
    dismissed.session !== session ||
    kept.length !== dismissed.versions.length
  ) {
    setDismissed({ session, versions: kept });
  }
  const shown = limits.filter(
    ({ notice }) => !kept.includes(versionOf(notice)),
  );
  if (shown.length === 0) return null;
  return (
    <ul className="usage-limit-banners" aria-label="Usage limits">
      {shown.map(({ notice, limit }) => {
        const reached = notice.level === "error";
        const text = usageLimitText(reached, limit);
        const Icon = reached ? CircleAlert : TriangleAlert;
        return (
          <li
            key={notice.identity}
            className={`usage-limit-banner usage-limit-banner--${notice.level}`}
            role={reached ? "alert" : "status"}
          >
            <Icon size={14} />
            <span className="usage-limit-banner__text">{text}</span>
            <button
              type="button"
              className="usage-limit-banner__dismiss"
              aria-label={`Dismiss: ${text}`}
              onClick={() =>
                setDismissed({
                  session,
                  versions: [...kept, versionOf(notice)],
                })
              }
            >
              ×
            </button>
          </li>
        );
      })}
    </ul>
  );
}
