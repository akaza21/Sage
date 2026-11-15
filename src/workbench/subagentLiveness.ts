import {
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";

import type { AcpChatPage } from "../domain/acpProjection";
import { isTauriRuntime } from "../ports/nativeInvoke";
import {
  getAcpChildTranscript,
  subscribeChildProjectionChanges,
  type ChildTranscript,
  type ChildTranscriptRequest,
} from "../services/acpChildTranscript";
import type { RecentAgent } from "./agentArtifactTabPresentation";

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

/** A live child re-reads at most once per this window, and within it. */
const CHILD_REFRESH_MS = 1_000;

/** Single-flight, throttled reads for one child. */
type ChildRefresh = {
  inFlight: boolean;
  /** The epoch moved while a read was in flight. */
  again: boolean;
  timer: ReturnType<typeof setTimeout> | undefined;
  /** When the latest read started (performance.now()). */
  startedAt: number;
  send: () => void;
};

function childRefresh(): ChildRefresh {
  return {
    inFlight: false,
    again: false,
    timer: undefined,
    startedAt: -Infinity,
    send: () => undefined,
  };
}

/** Leading/trailing throttle; never postpone a pending read during activity. */
function scheduleChildRefresh(state: ChildRefresh) {
  if (state.timer !== undefined) return;
  const wait = state.startedAt + CHILD_REFRESH_MS - performance.now();
  if (wait <= 0) return state.send();
  state.timer = setTimeout(() => state.send(), wait);
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
  const request = {
    projectId: page.projectId,
    projectGeneration,
    chatId: page.chatId,
    sessionId: turn.sessionId,
    runtimeEpoch: turn.runtimeEpoch,
    itemId,
    agentKey,
  };
  // The child projection epoch is this identity's invalidation token.
  return {
    identity: [
      request.projectId,
      projectGeneration,
      request.chatId,
      request.sessionId,
      request.runtimeEpoch,
      itemId,
      agentKey,
    ].join("\u0000"),
    request,
  };
}

type Settlement = "done" | "stopped" | "failed";

/** What a child transcript says once its last turn has settled. */
function transcriptSettlement(
  turns: readonly { status: string }[],
): Settlement | null {
  const status = turns.at(-1)?.status;
  return status === "completed"
    ? "done"
    : status === "interrupted"
      ? "stopped"
      : status === "failed"
        ? "failed"
        : null;
}

const MAX_SETTLEMENTS = 64;
const MAX_WATCHED_AGENTS = 8;
const MAX_WATCHES = 32;
const MAX_STALE_RETRIES = 3;

/**
 * Settled children by read identity and the ended parent turn that last
 * reported them, so a later parent report about the same child starts over.
 */
const settlements = new Map<string, Settlement>();
const listeners = new Set<() => void>();
let settlementVersion = 0;

function settlementKey(identity: string, turnId: string) {
  return identity + "\u0000" + turnId;
}

function noteSettlement(key: string, turns: readonly { status: string }[]) {
  const settlement = transcriptSettlement(turns);
  if (!settlement || settlements.get(key) === settlement) return;
  settlements.delete(key);
  settlements.set(key, settlement);
  if (settlements.size > MAX_SETTLEMENTS)
    settlements.delete(settlements.keys().next().value!);
  settlementVersion += 1;
  for (const listener of listeners) listener();
}

function currentSettlementVersion() {
  return settlementVersion;
}

function subscribeSettlements(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export type ChildReadResult = ChildTranscript | "error";

/** One mounted view's interest in a child's reads. */
type Reader = {
  identity: string;
  /** The child epoch this reader has asked to be served. */
  epoch: number | null;
  wanted: boolean;
  onRead: (value: ChildReadResult) => void;
};

/** Every view of one child shares this reader: one read at a time, throttled. */
type Watch = {
  read: ChildRead;
  readers: Set<Reader>;
  refresh: ChildRefresh;
  /** The epoch the latest scheduled read serves. */
  epoch: number | null;
  stale: number;
};

const watches = new Map<string, Watch>();
/** One child read in flight across all views; the others queue behind it. */
let reading: Watch | null = null;
const queued = new Set<Watch>();

/** Test isolation: forgets every watched child and settled result. */
export function resetSubagentLiveness() {
  for (const watch of watches.values()) clearTimeout(watch.refresh.timer);
  watches.clear();
  queued.clear();
  reading = null;
  settlements.clear();
}

function watchFor(read: ChildRead): Watch {
  const existing = watches.get(read.identity);
  if (existing) {
    existing.read = read;
    return existing;
  }
  for (const [identity, idle] of watches) {
    if (watches.size < MAX_WATCHES) break;
    if (idle.readers.size > 0 || idle.refresh.inFlight || queued.has(idle))
      continue;
    clearTimeout(idle.refresh.timer);
    watches.delete(identity);
  }
  const watch: Watch = {
    read,
    readers: new Set(),
    refresh: childRefresh(),
    epoch: null,
    stale: 0,
  };
  const state = watch.refresh;
  state.send = () => {
    state.timer = undefined;
    // A view that unmounted or no longer needs reads never reads.
    if (![...watch.readers].some((reader) => reader.wanted)) {
      watch.epoch = null;
      return;
    }
    if (state.inFlight) {
      state.again = true;
      return;
    }
    if (reading && reading !== watch) {
      queued.add(watch);
      return;
    }
    reading = watch;
    state.inFlight = true;
    state.startedAt = performance.now();
    // A user config, goal or fork command that meets a read in flight waits
    // for it in Rust (RuntimeHost::native_rpc), so reads cannot starve it.
    void getAcpChildTranscript(watch.read.request)
      // Only malformed successful responses throw; request failures are typed.
      .catch(() => "error" as const)
      .then((value) => {
        state.inFlight = false;
        if (reading !== watch) return;
        reading = null;
        const stale = typeof value === "object" && value.kind === "stale";
        watch.stale = stale ? watch.stale + 1 : 0;
        for (const reader of [...watch.readers]) reader.onRead(value);
        if (state.again || (stale && watch.stale < MAX_STALE_RETRIES)) {
          state.again = false;
          scheduleChildRefresh(state);
        }
        for (const next of queued) {
          if (reading) break;
          queued.delete(next);
          next.refresh.send();
        }
      });
  };
  watches.set(read.identity, watch);
  return watch;
}

/** Joins a child's shared reader and asks for the epoch when wanted. */
function attach(read: ChildRead, reader: Reader, epoch: number) {
  const watch = watchFor(read);
  watch.readers.add(reader);
  if (reader.wanted && reader.epoch !== epoch) {
    const fresh = reader.epoch === null;
    reader.epoch = epoch;
    // A read already sent for this epoch serves every view; a view that
    // just opened needs one it can see.
    if (watch.epoch !== epoch || (fresh && !watch.refresh.inFlight)) {
      watch.epoch = epoch;
      scheduleChildRefresh(watch.refresh);
    }
  }
  return () => {
    watch.readers.delete(reader);
  };
}

/** Reads one child for an open view on each child epoch, through the shared reader. */
export function useChildReads(
  read: ChildRead | null,
  epoch: number | null,
  wanted: boolean,
  onRead: (value: ChildReadResult) => void,
) {
  const readers = useRef<Reader | null>(null);
  useEffect(() => {
    if (!read || epoch === null) return;
    let reader = readers.current;
    if (reader?.identity !== read.identity)
      reader = readers.current = {
        identity: read.identity,
        epoch: null,
        wanted,
        onRead,
      };
    reader.wanted = wanted;
    reader.onRead = onRead;
    return attach(read, reader, epoch);
  }, [read, epoch, wanted, onRead]);
}

type Unconfirmed = RecentAgent & {
  unconfirmed: NonNullable<RecentAgent["unconfirmed"]>;
  target: NonNullable<RecentAgent["target"]>;
};

/**
 * Whether the runtime that ran an ended turn is gone: its session is no
 * longer current or has since closed or restarted its runtime.
 */
function runtimeEnded(page: AcpChatPage, turnId: string) {
  const turn = page.turns.find(({ id }) => id === turnId);
  const session = page.sessions?.find(({ id }) => id === turn?.sessionId);
  return (
    !!turn &&
    !!session &&
    ((page.currentSessionId !== undefined &&
      session.id !== page.currentSessionId) ||
      session.runtimeEpoch !== turn.runtimeEpoch)
  );
}

/**
 * Corrects subagents whose parent rows can no longer change: a child still
 * reported working from an ended parent turn takes its status from its own
 * transcript once that settles, and is stopped once the runtime that ran it
 * is gone, since its harness process went with it. Reads happen only while
 * a view is mounted, on child epoch changes, for the latest few such
 * children. Pass stable `agents` so a render does not re-attach every watch.
 */
export function useSettledSubagents(
  page: AcpChatPage,
  projectGeneration: string | undefined,
  agents: readonly RecentAgent[],
  epoch: number | null,
): RecentAgent[] {
  useSyncExternalStore(subscribeSettlements, currentSettlementVersion);
  const readers = useRef(new Map<string, Reader>());
  const ended = useMemo(
    () =>
      new Set(
        agents
          .filter(
            ({ unconfirmed }) =>
              unconfirmed && runtimeEnded(page, unconfirmed.turnId),
          )
          .map(({ id }) => id),
      ),
    [page, agents],
  );
  const watched = useMemo(
    () =>
      agents
        .filter(
          (agent): agent is Unconfirmed =>
            !!agent.unconfirmed && !!agent.target && !ended.has(agent.id),
        )
        .slice(-MAX_WATCHED_AGENTS)
        .flatMap((agent) => {
          const read = childReadFor(
            page,
            projectGeneration,
            agent.target.itemId,
            agent.target.agentKey,
          );
          return read
            ? [
                {
                  agent,
                  read,
                  key: settlementKey(read.identity, agent.unconfirmed.turnId),
                },
              ]
            : [];
        }),
    [page, projectGeneration, agents, ended],
  );
  useEffect(() => {
    if (epoch === null) return;
    const kept = new Map<string, Reader>();
    const releases = watched.map(({ read, key }) => {
      let reader = readers.current.get(key);
      if (!reader) {
        const created: Reader = {
          identity: read.identity,
          epoch: null,
          wanted: true,
          onRead: (value) => {
            if (typeof value === "object" && "turns" in value)
              noteSettlement(key, value.turns);
            created.wanted = !settlements.has(key);
          },
        };
        reader = created;
      }
      reader.wanted = !settlements.has(key);
      kept.set(key, reader);
      return attach(read, reader, epoch);
    });
    readers.current = kept;
    return () => releases.forEach((release) => release());
  }, [watched, epoch]);
  const settled = new Map(
    watched.flatMap(({ agent, key }) => {
      const settlement = settlements.get(key);
      return settlement ? [[agent.id, settlement] as const] : [];
    }),
  );
  return agents.map((agent) => {
    const status = ended.has(agent.id) ? "stopped" : settled.get(agent.id);
    return status ? { ...agent, status, endUnknown: true } : agent;
  });
}
