import {
  useCallback,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

import type { TurnProjection } from "../../../domain/acp";

const ESTIMATED_ROW_HEIGHT = 264;
const ROW_GAP = 24;

function firstOffsetAtOrAfter(offsets: readonly number[], target: number) {
  let low = 0;
  let high = offsets.length - 1;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (offsets[middle]! < target) low = middle + 1;
    else high = middle;
  }
  return low;
}

function MeasuredTurnRow({
  turn,
  index,
  count,
  onMeasure,
  children,
}: {
  turn: TurnProjection;
  index: number;
  count: number;
  onMeasure: (id: string, height: number) => void;
  children: ReactNode;
}) {
  const rowRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const row = rowRef.current;
    if (!row) return;
    const measure = () =>
      onMeasure(turn.id, row.getBoundingClientRect().height);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(row);
    return () => observer.disconnect();
  }, [onMeasure, turn.id]);
  return (
    <div
      ref={rowRef}
      className="acp-virtual-turn-row"
      role="listitem"
      aria-posinset={index + 1}
      aria-setsize={count}
    >
      {children}
    </div>
  );
}

export function VirtualTurnList({
  turns,
  renderTurn,
}: {
  turns: readonly TurnProjection[];
  renderTurn: (turn: TurnProjection, index: number) => ReactNode;
}) {
  const listRef = useRef<HTMLDivElement>(null);
  const [measured, setMeasured] = useState(() => new Map<string, number>());
  const latestHeights = useRef(new Map<string, number>());
  const previousIds = useRef<readonly string[]>([]);
  const anchor = useRef<{ id: string; inset: number } | null>(null);
  const pinnedAnchor = useRef<{ id: string; inset: number } | null>(null);
  const releaseFrame = useRef<number | null>(null);
  const [viewport, setViewport] = useState({ top: Infinity, height: 700 });
  const offsets = useMemo(() => {
    const values = [0];
    for (const turn of turns)
      values.push(
        values.at(-1)! +
          (measured.get(turn.id) ?? ESTIMATED_ROW_HEIGHT) +
          ROW_GAP,
      );
    return values;
  }, [measured, turns]);
  const totalHeight = offsets.at(-1) ?? 0;
  const top = Number.isFinite(viewport.top)
    ? Math.max(0, viewport.top)
    : Math.max(0, totalHeight - viewport.height);
  const overscan = viewport.height;
  const start = Math.max(
    0,
    firstOffsetAtOrAfter(offsets, Math.max(0, top - overscan)) - 1,
  );
  const end = Math.min(
    turns.length,
    firstOffsetAtOrAfter(offsets, top + viewport.height + overscan) + 1,
  );

  useLayoutEffect(() => {
    const ids = new Set<string>(turns.map((turn) => turn.id));
    for (const id of latestHeights.current.keys())
      if (!ids.has(id)) latestHeights.current.delete(id);
  }, [turns]);

  const readViewport = useCallback(() => {
    const list = listRef.current;
    const scrollport = list?.closest<HTMLElement>(".message-list");
    if (!list || !scrollport) return;
    const listTop =
      list.getBoundingClientRect().top -
      scrollport.getBoundingClientRect().top +
      scrollport.scrollTop;
    setViewport({
      top: scrollport.scrollTop - listTop,
      height: scrollport.clientHeight,
    });
  }, []);

  useLayoutEffect(() => {
    const scrollport = listRef.current?.closest<HTMLElement>(".message-list");
    if (!scrollport) return;
    readViewport();
    scrollport.addEventListener("scroll", readViewport, { passive: true });
    const observer =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(readViewport);
    observer?.observe(scrollport);
    return () => {
      scrollport.removeEventListener("scroll", readViewport);
      observer?.disconnect();
    };
  }, [readViewport]);

  useLayoutEffect(() => {
    const first = turns[0]?.id;
    const previous = previousIds.current;
    previousIds.current = turns.map((turn) => turn.id);
    if (!first || previous.length === 0 || first === previous[0]) return;
    const saved = anchor.current;
    const index = saved ? turns.findIndex((turn) => turn.id === saved.id) : -1;
    const scrollport = listRef.current?.closest<HTMLElement>(".message-list");
    if (!saved || index < 0 || !scrollport) return;
    pinnedAnchor.current = saved;
    const list = listRef.current!;
    const anchoredRow = [
      ...list.querySelectorAll<HTMLElement>(".acp-turn"),
    ].find((row) => row.dataset.turnId === saved.id);
    if (anchoredRow) {
      scrollport.scrollTop +=
        anchoredRow.getBoundingClientRect().top -
        scrollport.getBoundingClientRect().top -
        saved.inset;
      readViewport();
      return;
    }
    const listTop =
      list.getBoundingClientRect().top -
      scrollport.getBoundingClientRect().top +
      scrollport.scrollTop;
    scrollport.scrollTop = listTop + offsets[index]! - saved.inset;
    readViewport();
  }, [offsets, readViewport, turns]);

  useLayoutEffect(() => {
    const saved = pinnedAnchor.current;
    const list = listRef.current;
    const scrollport = list?.closest<HTMLElement>(".message-list");
    if (!saved || !list || !scrollport) return;
    const row = [...list.querySelectorAll<HTMLElement>(".acp-turn")].find(
      (element) => element.dataset.turnId === saved.id,
    );
    if (row) {
      const difference =
        row.getBoundingClientRect().top -
        scrollport.getBoundingClientRect().top -
        saved.inset;
      if (Math.abs(difference) >= 1) {
        scrollport.scrollTop += difference;
        readViewport();
      }
    }
    if (releaseFrame.current !== null)
      cancelAnimationFrame(releaseFrame.current);
    releaseFrame.current = requestAnimationFrame(() => {
      releaseFrame.current = requestAnimationFrame(() => {
        pinnedAnchor.current = null;
        releaseFrame.current = null;
      });
    });
  }, [end, measured, readViewport, start, turns]);

  useLayoutEffect(
    () => () => {
      if (releaseFrame.current !== null)
        cancelAnimationFrame(releaseFrame.current);
    },
    [],
  );

  useLayoutEffect(() => {
    const list = listRef.current;
    const scrollport = list?.closest<HTMLElement>(".message-list");
    if (!list || !scrollport || pinnedAnchor.current) return;
    const top = scrollport.getBoundingClientRect().top;
    const row = [...list.querySelectorAll<HTMLElement>(".acp-turn")].find(
      (element) => element.getBoundingClientRect().bottom > top + 16,
    );
    if (row?.dataset.turnId)
      anchor.current = {
        id: row.dataset.turnId,
        inset: row.getBoundingClientRect().top - top,
      };
  }, [end, measured, start, turns, viewport.top]);

  const onMeasure = useCallback(
    (id: string, height: number) => {
      if (height <= 0 || !Number.isFinite(height)) return;
      const prior =
        latestHeights.current.get(id) ??
        measured.get(id) ??
        ESTIMATED_ROW_HEIGHT;
      if (Math.abs(prior - height) < 1) return;
      latestHeights.current.set(id, height);
      setMeasured((current) => {
        const ids = new Set<string>(turns.map((turn) => turn.id));
        const next = new Map([...current].filter(([rowId]) => ids.has(rowId)));
        next.set(id, height);
        return next;
      });
      const index = turns.findIndex((turn) => turn.id === id);
      const scrollport = listRef.current?.closest<HTMLElement>(".message-list");
      if (
        scrollport &&
        index >= 0 &&
        offsets[index]! + prior + ROW_GAP <= viewport.top
      )
        scrollport.scrollTop += height - prior;
    },
    [measured, offsets, turns, viewport.top],
  );

  return (
    <div
      ref={listRef}
      className="acp-virtual-turns"
      role="list"
      aria-label="Chat turns"
    >
      <div aria-hidden="true" style={{ height: offsets[start] }} />
      {turns.slice(start, end).map((turn, offset) => (
        <MeasuredTurnRow
          key={turn.id}
          turn={turn}
          index={start + offset}
          count={turns.length}
          onMeasure={onMeasure}
        >
          {renderTurn(turn, start + offset)}
        </MeasuredTurnRow>
      ))}
      <div
        aria-hidden="true"
        style={{ height: Math.max(0, totalHeight - offsets[end]!) }}
      />
    </div>
  );
}
