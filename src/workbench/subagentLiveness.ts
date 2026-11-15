import {
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";

import type { AcpChatPage, TurnProjection } from "../domain/acpProjection";
import { terminalTurnStates } from "../features/chat/acp/statusSemantics";
import { isTauriRuntime } from "../ports/nativeInvoke";
import {
  getAcpChildTranscript,
  subscribeChildProjectionChanges,
  type ChildTranscript,
  type ChildTranscriptRequest,
} from "../services/acpChildTranscript";
import {
  liveAgent,
  MAX_RESOURCE_ENTRIES,
  type RecentAgent,
} from "./agentArtifactTabPresentation";

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

function send(watch: Watch) {
  watch.timer = undefined;
  // A read in flight reschedules once it lands if a reader is still behind.
  if (watch.inFlight || !behind(watch)) return;
  if (reading) {
    queued.add(watch);
    return;
  }
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
      for (const next of queued) {
        if (reading) break;
        queued.delete(next);
        send(next);
      }
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

type Outcome = "done" | "stopped" | "failed";

/** Every recent child of both visible chats keeps its known outcome. */
const MAX_OUTCOMES = 2 * MAX_RESOURCE_ENTRIES;
/** Children one view reads per child epoch, least recently read first. */
const MAX_READS_PER_EPOCH = 8;

/**
 * Known outcomes, oldest first, by child and the ended parent turn that
 * reported it live, so a later parent report about the child starts over.
 * Each change replaces the map, so a render sees one consistent snapshot.
 */
let outcomes: ReadonlyMap<string, Outcome> = new Map();
const outcomeListeners = new Set<() => void>();

function noteOutcome(key: string, outcome: Outcome) {
  if (outcomes.get(key) === outcome) return;
  const next = new Map(outcomes);
  next.delete(key);
  next.set(key, outcome);
  if (next.size > MAX_OUTCOMES) next.delete(next.keys().next().value!);
  outcomes = next;
  for (const listener of outcomeListeners) listener();
}

function subscribeOutcomes(listener: () => void) {
  outcomeListeners.add(listener);
  return () => outcomeListeners.delete(listener);
}

const currentOutcomes = () => outcomes;

/** Test isolation: forgets every watched child and known outcome. */
export function resetSubagentLiveness() {
  for (const watch of watches.values()) clearTimeout(watch.timer);
  watches.clear();
  queued.clear();
  reading = null;
  outcomes = new Map();
}

/**
 * What a read says about a child. A transcript speaks once its last turn
 * has settled. Once the runtime that ran the child is gone, a transcript
 * still in progress, or none at all, means the child stopped with it.
 */
function readOutcome(value: ChildReadResult, ended: boolean): Outcome | null {
  if (typeof value === "object" && "turns" in value) {
    switch (value.turns.at(-1)?.status) {
      case "completed":
        return "done";
      case "interrupted":
        return "stopped";
      case "failed":
        return "failed";
    }
    return value.kind === "fromEarlierSession" ? "stopped" : null;
  }
  return ended && !(typeof value === "object" && value.kind === "stale")
    ? "stopped"
    : null;
}

/**
 * The ended parent turn whose row still reports a child live: no later
 * parent row can change it, so only its own transcript can.
 */
export function unconfirmedTurn(agent: RecentAgent): TurnProjection | null {
  const turn = agent.reportedTurn;
  return turn && liveAgent(agent.status) && terminalTurnStates.has(turn.state)
    ? turn
    : null;
}

/**
 * Whether the runtime that ran an ended turn is gone: its session is no
 * longer current or has since closed or restarted its runtime.
 */
function runtimeEnded(page: AcpChatPage, turn: TurnProjection) {
  const session = page.sessions?.find(({ id }) => id === turn.sessionId);
  return (
    !!session &&
    ((page.currentSessionId !== undefined &&
      session.id !== page.currentSessionId) ||
      session.runtimeEpoch !== turn.runtimeEpoch)
  );
}

type UnconfirmedChild = {
  id: string;
  /** The child's key in `outcomes`. */
  key: string;
  ended: boolean;
  read: ChildRead | null;
};

/**
 * Corrects subagents whose parent rows can no longer change: a child still
 * reported live from an ended parent turn takes the outcome its own
 * transcript gives, live or stored, and one whose runtime is gone without a
 * readable transcript stopped with it. Known outcomes always apply. Reads
 * happen only while a view is mounted, on child epoch changes, for a
 * bounded share of the unresolved children, least recently read first.
 * Pass stable `agents` so a render does not re-attach every read.
 */
export function useSettledSubagents(
  page: AcpChatPage,
  projectGeneration: string | undefined,
  agents: readonly RecentAgent[],
  epoch: number | null,
): RecentAgent[] {
  const known = useSyncExternalStore(subscribeOutcomes, currentOutcomes);
  const children = useMemo(
    () =>
      agents.flatMap((agent): UnconfirmedChild[] => {
        const turn = unconfirmedTurn(agent);
        if (!turn) return [];
        const { target } = agent;
        return [
          {
            id: agent.id,
            key: [page.projectId, page.chatId, turn.id, agent.id].join(
              "\u0000",
            ),
            ended: runtimeEnded(page, turn),
            read: target
              ? childReadFor(
                  page,
                  projectGeneration,
                  target.itemId,
                  target.agentKey,
                )
              : null,
          },
        ];
      }),
    [page, projectGeneration, agents],
  );
  const unresolved = useMemo(
    () =>
      children.filter(
        (child): child is UnconfirmedChild & { read: ChildRead } =>
          !!child.read && !known.has(child.key),
      ),
    [children, known],
  );
  const readers = useRef(new Map<string, Reader>());
  useEffect(() => {
    if (epoch === null) return;
    const kept = new Map<string, Reader>();
    const due = unresolved.map(({ key, read, ended }) => {
      const reader: Reader = readers.current.get(key) ?? {
        served: null,
        wanted: true,
        onRead: () => undefined,
      };
      reader.onRead = (value) => {
        const outcome = readOutcome(value, ended);
        if (!outcome) return;
        reader.wanted = false;
        noteOutcome(key, outcome);
      };
      kept.set(key, reader);
      return { read, reader };
    });
    readers.current = kept;
    const releases = due
      .sort((a, b) => (a.reader.served ?? -1) - (b.reader.served ?? -1))
      .slice(0, MAX_READS_PER_EPOCH)
      .map(({ read, reader }) => attach(read, reader, epoch));
    return () => releases.forEach((release) => release());
  }, [unresolved, epoch]);
  return agents.map((agent) => {
    const child = children.find(({ id }) => id === agent.id);
    const status =
      child &&
      (known.get(child.key) ?? (child.ended && !child.read ? "stopped" : null));
    return status ? { ...agent, status, endUnknown: true } : agent;
  });
}
