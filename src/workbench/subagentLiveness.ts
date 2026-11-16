import { useEffect, useRef, useState } from "react";

import type { AcpChatPage } from "../domain/acpProjection";
import { isTauriRuntime } from "../ports/nativeInvoke";
import {
  getAcpChildTranscript,
  subscribeChildProjectionChanges,
  type ChildTranscript,
  type ChildTranscriptRequest,
} from "../services/acpChildTranscript";

/** Latest child epoch; desktop reads wait for listener attachment to avoid gaps. Non-desktop reads never wait. */
export function useChildEpoch(page: AcpChatPage): number | null {
  const [notice, setNotice] = useState({ chat: "", epoch: 0, attached: false });
  const chat = page.projectId + "\u0000" + page.chatId;
  useEffect(() => {
    let live = true;
    let stop = () => {};
    const attached = () => {
      if (live)
        setNotice((previous) =>
          previous.chat === chat
            ? { ...previous, attached: true }
            : { chat, epoch: 0, attached: true },
        );
    };
    // Ignore notices from torn-down Chat listeners.
    subscribeChildProjectionChanges(page.projectId, page.chatId, (epoch) => {
      if (live) setNotice({ chat, epoch, attached: true });
    }).then(
      (unlisten) => {
        if (!live) return unlisten();
        stop = unlisten;
        attached();
      },
      // Without a listener, use page epochs.
      attached,
    );
    return () => {
      live = false;
      stop();
    };
  }, [chat, page.projectId, page.chatId]);
  const own = page.childProjectionEpoch ?? 0;
  const listenerless = !isTauriRuntime();
  if (notice.chat === chat && (notice.attached || listenerless))
    return Math.max(own, notice.epoch);
  return listenerless ? own : null;
}

export type ChildRead = { identity: string; request: ChildTranscriptRequest };

/** The verified child read behind a spawn row, or null when none may be sent. */
export function childReadFor(
  page: AcpChatPage,
  projectGeneration: string | undefined,
  itemId: string,
  agentKey: string | undefined,
): ChildRead | null {
  const turn = page.turns.find((candidate) =>
    candidate.items.some((item) => item.id === itemId),
  );
  const item = turn?.items.find((candidate) => candidate.id === itemId);
  if (
    !turn ||
    item?.kind !== "collaboration" ||
    !projectGeneration ||
    !agentKey
  )
    return null;
  const session = page.sessions?.find(
    (candidate) => candidate.id === turn.sessionId,
  );
  const keys = item.payload.agentKeys ?? [];
  const attributedKey =
    keys.length === 1 &&
    (!item.payload.agentKey || item.payload.agentKey === keys[0])
      ? keys[0]
      : keys.length === 0
        ? item.payload.agentKey
        : null;
  if (
    (page.currentSessionId !== undefined &&
      page.currentSessionId !== turn.sessionId) ||
    !session?.capabilities?.childTranscriptRead ||
    attributedKey !== agentKey ||
    (session.runtimeEpoch !== null &&
      session.runtimeEpoch !== turn.runtimeEpoch)
  )
    return null;
  const request: ChildTranscriptRequest = {
    projectId: page.projectId,
    projectGeneration,
    chatId: page.chatId,
    sessionId: turn.sessionId,
    runtimeEpoch: turn.runtimeEpoch,
    itemId,
    agentKey,
  };
  // The child projection epoch is this identity's invalidation token.
  return { identity: Object.values(request).join("\u0000"), request };
}

export type ChildReadResult = ChildTranscript | "error";

/** Consecutive stale answers one read retries before waiting for a new epoch. */
export const MAX_STALE_RETRIES = 3;
/** A child re-reads at most once per this window, and within it. */
const CHILD_REFRESH_MS = 1_000;
/** Idle watches are dropped once this many children are watched. */
const MAX_WATCHES = 32;

/** One mounted view's interest in a child's reads. */
type Reader = {
  /** The child epoch of the last read this reader was given. */
  served: number | null;
  wanted: boolean;
  onRead: (value: ChildReadResult) => void;
};

/** Every view of one child shares this: one read at a time, throttled. */
type Watch = {
  read: ChildRead;
  readers: Set<Reader>;
  /** The newest child epoch a reader asked for. */
  epoch: number;
  inFlight: boolean;
  timer: ReturnType<typeof setTimeout> | undefined;
  /** When the latest read started (performance.now()). */
  startedAt: number;
  stale: number;
};

const watches = new Map<string, Watch>();
/** One child read in flight across all views; the others queue behind it. */
let reading: Watch | null = null;
const queued = new Set<Watch>();

function watchFor(read: ChildRead): Watch {
  const existing = watches.get(read.identity);
  if (existing) {
    existing.read = read;
    return existing;
  }
  for (const [identity, idle] of watches) {
    if (watches.size < MAX_WATCHES) break;
    if (idle.readers.size > 0 || idle.inFlight || queued.has(idle)) continue;
    clearTimeout(idle.timer);
    watches.delete(identity);
  }
  const watch: Watch = {
    read,
    readers: new Set(),
    epoch: 0,
    inFlight: false,
    timer: undefined,
    startedAt: -Infinity,
    stale: 0,
  };
  watches.set(read.identity, watch);
  return watch;
}

/** Whether a reader that wants reads has not yet seen the newest epoch. */
function behind(watch: Watch) {
  return [...watch.readers].some(
    (reader) => reader.wanted && reader.served !== watch.epoch,
  );
}

/** Leading/trailing throttle; never postpone a pending read during activity. */
function schedule(watch: Watch) {
  if (watch.timer !== undefined) return;
  const wait = watch.startedAt + CHILD_REFRESH_MS - performance.now();
  if (wait <= 0) send(watch);
  else watch.timer = setTimeout(() => send(watch), wait);
}

/** Queues a due read behind the reads already waiting, in arrival order. */
function send(watch: Watch) {
  watch.timer = undefined;
  // A read in flight reschedules once it lands if a reader is still behind.
  if (watch.inFlight || !behind(watch)) return;
  queued.add(watch);
  dispatch();
}

/** Starts the longest-waiting due read once no read is in flight. */
function dispatch() {
  for (const watch of queued) {
    if (reading) return;
    queued.delete(watch);
    if (!watch.inFlight && behind(watch)) start(watch);
  }
}

function start(watch: Watch) {
  reading = watch;
  watch.inFlight = true;
  watch.startedAt = performance.now();
  const epoch = watch.epoch;
  // A user config, goal or fork command that meets a read in flight waits
  // for it in Rust (RuntimeHost::native_rpc), so reads cannot starve it.
  void getAcpChildTranscript(watch.read.request)
    // Only malformed successful responses throw; request failures are typed.
    .catch(() => "error" as const)
    .then((value) => {
      watch.inFlight = false;
      if (reading !== watch) return;
      reading = null;
      const stale = typeof value === "object" && value.kind === "stale";
      watch.stale = stale ? watch.stale + 1 : 0;
      const retry = stale && watch.stale < MAX_STALE_RETRIES;
      for (const reader of [...watch.readers]) {
        if (!retry) reader.served = epoch;
        reader.onRead(value);
      }
      if (behind(watch)) schedule(watch);
      dispatch();
    });
}

/** Joins a child's shared reads, asking for one at `epoch` when wanted. */
function attach(read: ChildRead, reader: Reader, epoch: number) {
  const watch = watchFor(read);
  watch.readers.add(reader);
  if (reader.wanted && reader.served !== epoch) {
    watch.epoch = epoch;
    schedule(watch);
  }
  return () => {
    watch.readers.delete(reader);
  };
}

/** Reads one child for an open view on each child epoch, through the shared reads. */
export function useChildReads(
  read: ChildRead | null,
  epoch: number | null,
  wanted: boolean,
  onRead: (value: ChildReadResult) => void,
) {
  const owned = useRef<{ identity: string; reader: Reader } | null>(null);
  useEffect(() => {
    if (!read || epoch === null) return;
    if (owned.current?.identity !== read.identity)
      owned.current = {
        identity: read.identity,
        reader: { served: null, wanted, onRead },
      };
    const { reader } = owned.current;
    reader.wanted = wanted;
    reader.onRead = onRead;
    return attach(read, reader, epoch);
  }, [read, epoch, wanted, onRead]);
}

/** Test isolation: forgets every watched child. */
export function resetSubagentLiveness() {
  for (const watch of watches.values()) clearTimeout(watch.timer);
  watches.clear();
  queued.clear();
  reading = null;
}
