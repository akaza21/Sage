import { useEffect, useMemo, useState, useSyncExternalStore } from "react";

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
export type ChildRefresh = {
  identity: string;
  inFlight: boolean;
  /** The epoch moved while a read was in flight. */
  again: boolean;
  timer: ReturnType<typeof setTimeout> | undefined;
  /** When the latest read started (performance.now()). */
  startedAt: number;
  send: () => void;
};

export function childRefresh(): ChildRefresh {
  return {
    identity: "",
    inFlight: false,
    again: false,
    timer: undefined,
    startedAt: -Infinity,
    send: () => undefined,
  };
}

/** Leading/trailing throttle; never postpone a pending read during activity. */
export function scheduleChildRefresh(state: ChildRefresh) {
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

const tails = new Map<string, Promise<unknown>>();

/** One read in flight per child across every view; a later read waits for it. */
export function readChildTranscript(
  read: ChildRead,
): Promise<ChildTranscript | "error"> {
  const send = () =>
    // Only malformed successful responses throw; request failures are typed.
    getAcpChildTranscript(read.request).catch(() => "error" as const);
  const prior = tails.get(read.identity);
  const next = prior ? prior.then(send) : send();
  tails.set(read.identity, next);
  void next.then(() => {
    if (tails.get(read.identity) === next) tails.delete(read.identity);
  });
  return next;
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

/**
 * Settled children by read identity and the terminal parent turn that last
 * reported them, so a later parent report about the same child starts over.
 */
const settlements = new Map<string, Settlement>();
const listeners = new Set<() => void>();
let settlementVersion = 0;

function settlementKey(identity: string, turnId: string) {
  return identity + "\u0000" + turnId;
}

/** Records what a child read found once its transcript has settled. */
export function noteChildSettlement(
  identity: string,
  turnId: string,
  turns: readonly { status: string }[],
) {
  const settlement = transcriptSettlement(turns);
  const key = settlementKey(identity, turnId);
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

/** Identities the open Subagent view is already reading. */
const claims = new Map<string, number>();

export function claimChildRead(identity: string) {
  claims.set(identity, (claims.get(identity) ?? 0) + 1);
  return () => {
    const count = (claims.get(identity) ?? 1) - 1;
    if (count > 0) claims.set(identity, count);
    else claims.delete(identity);
  };
}

type Watch = {
  read: ChildRead;
  turnId: string;
  users: number;
  epoch: number | null;
  refresh: ChildRefresh;
};

/** Watches persist across renders so the throttle survives effect re-runs. */
const watches = new Map<string, Watch>();

function watchFor(read: ChildRead, turnId: string): Watch {
  const existing = watches.get(read.identity);
  if (existing) {
    existing.read = read;
    if (existing.turnId !== turnId) existing.epoch = null;
    existing.turnId = turnId;
    return existing;
  }
  for (const [identity, idle] of watches) {
    if (watches.size < MAX_WATCHES) break;
    if (idle.users > 0 || idle.refresh.inFlight) continue;
    clearTimeout(idle.refresh.timer);
    watches.delete(identity);
  }
  const watch: Watch = {
    read,
    turnId,
    users: 0,
    epoch: null,
    refresh: childRefresh(),
  };
  const state = watch.refresh;
  state.send = () => {
    state.timer = undefined;
    // An unmounted view never reads; the next watcher starts over.
    if (watch.users === 0) {
      watch.epoch = null;
      return;
    }
    if (settlements.has(settlementKey(watch.read.identity, watch.turnId)))
      return;
    if (state.inFlight) {
      state.again = true;
      return;
    }
    state.inFlight = true;
    state.startedAt = performance.now();
    const { identity } = watch.read;
    const turnId = watch.turnId;
    void readChildTranscript(watch.read).then((value) => {
      state.inFlight = false;
      if (typeof value === "object" && "turns" in value)
        noteChildSettlement(identity, turnId, value.turns);
      if (state.again) {
        state.again = false;
        scheduleChildRefresh(state);
      }
    });
  };
  watches.set(read.identity, watch);
  return watch;
}

type Unconfirmed = RecentAgent & {
  unconfirmed: NonNullable<RecentAgent["unconfirmed"]>;
  target: NonNullable<RecentAgent["target"]>;
};

/**
 * Corrects subagents whose parent rows can no longer change: a child still
 * reported working from an ended parent turn takes its status from its own
 * transcript once that settles. Reads happen only while a view is mounted,
 * on child epoch changes, for the latest few such children.
 */
export function useSettledSubagents(
  page: AcpChatPage,
  projectGeneration: string | undefined,
  agents: readonly RecentAgent[],
  epoch: number | null,
): RecentAgent[] {
  useSyncExternalStore(subscribeSettlements, currentSettlementVersion);
  const watched = useMemo(
    () =>
      agents
        .filter(
          (agent): agent is Unconfirmed =>
            !!agent.unconfirmed && !!agent.target,
        )
        .slice(-MAX_WATCHED_AGENTS)
        .flatMap((agent) => {
          const read = childReadFor(
            page,
            projectGeneration,
            agent.target.itemId,
            agent.target.agentKey,
          );
          return read ? [{ agent, read }] : [];
        }),
    [page, projectGeneration, agents],
  );
  useEffect(() => {
    if (epoch === null) return;
    const releases = watched.map(({ agent, read }) => {
      const watch = watchFor(read, agent.unconfirmed.turnId);
      watch.users += 1;
      if (
        watch.epoch !== epoch &&
        !claims.has(read.identity) &&
        !settlements.has(settlementKey(read.identity, watch.turnId))
      ) {
        watch.epoch = epoch;
        scheduleChildRefresh(watch.refresh);
      }
      return () => {
        watch.users -= 1;
      };
    });
    return () => releases.forEach((release) => release());
  }, [watched, epoch]);
  const settled = new Map(
    watched.flatMap(({ agent, read }) => {
      const settlement = settlements.get(
        settlementKey(read.identity, agent.unconfirmed.turnId),
      );
      return settlement ? [[agent.id, settlement] as const] : [];
    }),
  );
  return agents.map((agent) => {
    const status = settled.get(agent.id);
    return status ? { ...agent, status, endUnknown: true } : agent;
  });
}
