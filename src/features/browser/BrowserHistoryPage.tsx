import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import type { BrowserVisit } from "../../ports/nativeBrowserPort";
import { Globe, Search, X } from "../../ui/leanIcons";
import { HISTORY_MAX_LOADED, useBrowsingHistory } from "./useBrowsingHistory";
import "./BrowserHistoryPage.css";

const timeFormat = new Intl.DateTimeFormat(undefined, {
  hour: "numeric",
  minute: "2-digit",
});
const dayFormat = new Intl.DateTimeFormat(undefined, {
  weekday: "long",
  month: "long",
  day: "numeric",
  year: "numeric",
});

function dayKey(time: number): string {
  const date = new Date(time);
  return `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`;
}

/** "Today", "Yesterday" or the full local date, as Chrome heads its days. */
export function historyDayLabel(time: number, now = Date.now()): string {
  const today = new Date(now);
  const yesterday = new Date(
    today.getFullYear(),
    today.getMonth(),
    today.getDate() - 1,
  );
  const key = dayKey(time);
  const full = dayFormat.format(time);
  if (key === dayKey(now)) return `Today - ${full}`;
  if (key === dayKey(yesterday.getTime())) return `Yesterday - ${full}`;
  return full;
}

function host(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

function groupByDay(visits: readonly BrowserVisit[]) {
  const days: { key: string; first: number; visits: BrowserVisit[] }[] = [];
  for (const visit of visits) {
    const key = dayKey(visit.visitedAt);
    const last = days.at(-1);
    if (last?.key === key) last.visits.push(visit);
    else days.push({ key, first: visit.visitedAt, visits: [visit] });
  }
  return days;
}

/**
 * The browser's History page (BF-7c): every page any browser tab
 * committed, newest first and grouped by day, with search and per-visit
 * removal. It covers this tab's page; choosing a visit opens it here.
 */
export function BrowserHistoryPage({
  onNavigate,
  onClose,
  clearDataControl,
}: {
  onNavigate: (url: string) => void;
  onClose: () => void;
  clearDataControl?: ReactNode;
}) {
  const [query, setQuery] = useState("");
  const search = query.trim();
  const history = useBrowsingHistory(search);
  const field = useRef<HTMLInputElement>(null);
  const headingId = useId();

  useEffect(() => field.current?.focus(), []);

  const days = groupByDay(history.visits);
  let status: string | null = null;
  if (history.loading) status = "Loading history…";
  else if (!history.visits.length && !history.error)
    status = search
      ? `No history matches “${search}”.`
      : "No browsing history yet. Pages you open in the browser appear here.";

  return (
    <section
      className="browser-history"
      aria-labelledby={headingId}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        event.preventDefault();
        event.stopPropagation();
        onClose();
      }}
    >
      <header className="browser-history__header">
        <h1 id={headingId}>History</h1>
        <button
          type="button"
          className="browser-history__close"
          aria-label="Close history"
          title="Close history"
          onClick={onClose}
        >
          <X size={14} aria-hidden="true" />
        </button>
      </header>
      <div className="browser-history__tools">
        <label className="browser-history__search">
          <Search size={14} aria-hidden="true" />
          <input
            ref={field}
            type="search"
            aria-label="Search history"
            placeholder="Search history"
            value={query}
            maxLength={256}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
        {clearDataControl}
      </div>
      {history.error ? (
        <p className="browser-history__error" role="alert">
          {history.error}
        </p>
      ) : null}
      {status ? (
        <p className="browser-history__status" role="status">
          {status}
        </p>
      ) : (
        days.map((day) => (
          <section key={day.key} className="browser-history__day">
            <h2>{historyDayLabel(day.first)}</h2>
            <ul>
              {day.visits.map((visit) => {
                const label = visit.title ?? visit.url;
                return (
                  <li key={visit.id} className="browser-history__visit">
                    <button
                      type="button"
                      className="browser-history__open"
                      title={visit.url}
                      onClick={() => onNavigate(visit.url)}
                    >
                      <time dateTime={new Date(visit.visitedAt).toISOString()}>
                        {timeFormat.format(visit.visitedAt)}
                      </time>
                      <Globe size={14} aria-hidden="true" />
                      <span className="browser-history__title">{label}</span>
                      <span className="browser-history__host">
                        {host(visit.url)}
                      </span>
                    </button>
                    <button
                      type="button"
                      className="browser-history__remove"
                      aria-label={`Remove ${label} from history`}
                      title="Remove from history"
                      disabled={history.removing.has(visit.id)}
                      onClick={() => history.remove(visit)}
                    >
                      <X size={14} aria-hidden="true" />
                    </button>
                  </li>
                );
              })}
            </ul>
          </section>
        ))
      )}
      {history.canLoadMore ? (
        <button
          type="button"
          className="browser-history__more"
          disabled={history.loadingMore}
          onClick={history.loadMore}
        >
          {history.loadingMore ? "Loading…" : "Show more"}
        </button>
      ) : null}
      {history.truncated ? (
        <p className="browser-history__status">
          Showing the newest {HISTORY_MAX_LOADED} visits. Search to find older
          ones.
        </p>
      ) : null}
    </section>
  );
}
