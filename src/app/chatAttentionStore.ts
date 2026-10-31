import { useSyncExternalStore } from "react";
import {
  chatAttention,
  type ChatAttentionPort,
} from "../services/chatAttention";
import {
  validPosition,
  type AttentionMembership,
  type ChatAttentionEvent,
  type ChatAttentionV1,
  type PresentedAttention,
} from "../domain/chatAttention";

export function createChatAttentionStore(port: ChatAttentionPort) {
  let values: ReadonlyMap<string, ChatAttentionV1> = new Map();
  let membership = new Map<string, ReadonlySet<string>>();
  let generation = 0;
  let unlisten: (() => void) | undefined;
  let retry: ReturnType<typeof setTimeout> | undefined;
  const listeners = new Set<() => void>();
  const wanted = new Map<string, string>();
  const loading = new Map<string, { again: boolean; promise: Promise<void> }>();
  const current = (row: ChatAttentionV1) =>
    membership.get(row.projectId)?.has(row.chatId) === true;
  const notify = () => {
    for (const listener of listeners) listener();
  };
  const accept = (rows: readonly ChatAttentionV1[], epoch: number) => {
    if (epoch !== generation || rows.length > 64) return;
    const next = new Map(values);
    for (const row of rows) {
      if (
        !current(row) ||
        !validPosition(row.revision) ||
        !validPosition(row.activityPosition)
      )
        continue;
      const minimum = wanted.get(row.chatId);
      const previous = next.get(row.chatId);
      if (
        (minimum && BigInt(row.revision) < BigInt(minimum)) ||
        (previous && BigInt(row.revision) <= BigInt(previous.revision))
      )
        continue;
      next.set(row.chatId, row);
    }
    values = next;
    notify();
  };
  const recover = () => {
    if (retry || !membership.size) return;
    const epoch = generation;
    retry = setTimeout(() => {
      retry = undefined;
      if (epoch === generation)
        void connect(
          [...membership].map(([projectId, ids]) => ({
            projectId,
            chatIds: [...ids],
          })),
        );
    }, 500);
  };
  const load = (projectId: string): Promise<void> => {
    const existing = loading.get(projectId);
    if (existing) {
      existing.again = true;
      return existing.promise;
    }
    const epoch = generation;
    const ids = [...(membership.get(projectId) ?? [])];
    const job = { again: false, promise: Promise.resolve() };
    job.promise = (async () => {
      do {
        job.again = false;
        try {
          accept(await port.get(projectId, ids), epoch);
        } catch {
          if (epoch === generation) recover();
        }
      } while (job.again && epoch === generation);
      if (epoch === generation) loading.delete(projectId);
    })();
    loading.set(projectId, job);
    return job.promise;
  };
  const refresh = async () => {
    await Promise.all([...membership.keys()].map(load));
  };
  const event = async (notice: ChatAttentionEvent) => {
    if ("resyncRequired" in notice) {
      await refresh();
      return;
    }
    if (!membership.get(notice.projectId)?.has(notice.chatId)) return;
    const previous =
      wanted.get(notice.chatId) ?? values.get(notice.chatId)?.revision;
    if (previous && BigInt(notice.revision) <= BigInt(previous)) return;
    wanted.set(notice.chatId, notice.revision);
    await load(notice.projectId);
  };
  const disconnect = () => {
    generation++;
    unlisten?.();
    unlisten = undefined;
    if (retry) clearTimeout(retry);
    retry = undefined;
    loading.clear();
    wanted.clear();
    membership.clear();
    values = new Map();
    notify();
  };
  async function connect(members: readonly AttentionMembership[]) {
    disconnect();
    if (
      members.length > 64 ||
      members.some((member) => member.chatIds.length > 64) ||
      members.reduce((count, member) => count + member.chatIds.length, 0) > 576
    )
      throw new Error("Attention membership exceeds bounds");
    membership = new Map(
      members.map((member) => [member.projectId, new Set(member.chatIds)]),
    );
    const epoch = generation;
    try {
      const dispose = await port.subscribe(async (notice) => {
        if (epoch === generation) await event(notice);
      });
      if (epoch !== generation) {
        dispose();
        return;
      }
      unlisten = dispose;
      await refresh();
    } catch {
      if (epoch === generation) recover();
    }
  }
  const ack = async (row: ChatAttentionV1, observedPosition: string) => {
    const epoch = generation;
    try {
      accept(
        [await port.ack(row.projectId, row.chatId, observedPosition)],
        epoch,
      );
    } catch {
      if (epoch === generation) recover();
    }
  };
  return {
    connect,
    disconnect,
    refresh,
    getSnapshot: () => values,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    async present(detail: PresentedAttention) {
      const row = values.get(detail.chatId);
      if (
        !row ||
        row.projectId !== detail.projectId ||
        !detail.presented ||
        !detail.foreground ||
        !detail.agent ||
        detail.settings ||
        (row.readPosition !== null &&
          BigInt(row.readPosition) >= BigInt(detail.observedPosition))
      )
        return;
      await ack(row, detail.observedPosition);
    },
    async markUnread(projectId: string, chatId: string) {
      const row = values.get(chatId);
      if (!row || row.projectId !== projectId) return;
      const epoch = generation;
      try {
        accept([await port.markUnread(projectId, chatId, row.revision)], epoch);
      } catch {
        if (epoch === generation) await load(projectId);
      }
    },
    async markAllRead() {
      for (const row of values.values()) await ack(row, row.activityPosition);
    },
  };
}

export const chatAttentionStore = createChatAttentionStore(chatAttention);
export const useChatAttention = () =>
  useSyncExternalStore(
    chatAttentionStore.subscribe,
    chatAttentionStore.getSnapshot,
  );
