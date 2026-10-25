import { create } from "zustand";

import { isShortId } from "../domain/guards";

import type { AgentArtifactOwner } from "../domain/agentArtifact";
import {
  sameAgentArtifactOwner,
  validAgentArtifactOwner,
} from "../domain/agentArtifact";

export const MAX_AGENT_ARTIFACT_FOLLOW_OWNERS = 2;
export const MAX_AGENT_ARTIFACT_TARGETS = 33;

export type AgentArtifactTarget =
  { kind: "tab"; tabId: string } | { kind: "preview" };

export type AgentArtifactFollowRecord = {
  owner: AgentArtifactOwner;
  mode: "follow" | "pinned";
  selected: AgentArtifactTarget | null;
  recent: readonly AgentArtifactTarget[];
  pending: readonly AgentArtifactTarget[];
  lastPreviewRevision: number;
};

export function agentArtifactPendingTargets(
  record: AgentArtifactFollowRecord | null | undefined,
): readonly AgentArtifactTarget[] {
  return record?.mode === "pinned" ? record.pending : [];
}

interface AgentArtifactFollowStore {
  records: readonly AgentArtifactFollowRecord[];
  observe: (
    owner: AgentArtifactOwner,
    target: AgentArtifactTarget,
    previewRevision?: number,
  ) => void;
  pin: (owner: AgentArtifactOwner, target: AgentArtifactTarget) => void;
  followLatest: (owner: AgentArtifactOwner) => void;
  reconcile: (
    owner: AgentArtifactOwner,
    available: readonly AgentArtifactTarget[],
  ) => void;
  forgetTab: (tabId: string) => void;
  clear: () => void;
}

export function validAgentArtifactTarget(
  target: AgentArtifactTarget | null | undefined,
): target is AgentArtifactTarget {
  return Boolean(
    target &&
    (target.kind === "preview" ||
      (target.kind === "tab" && isShortId(target.tabId))),
  );
}

export function sameAgentArtifactTarget(
  left: AgentArtifactTarget | null | undefined,
  right: AgentArtifactTarget | null | undefined,
): boolean {
  if (!left || !right || left.kind !== right.kind) return false;
  if (left.kind === "preview" && right.kind === "preview") return true;
  return (
    left.kind === "tab" && right.kind === "tab" && left.tabId === right.tabId
  );
}

export function agentArtifactFollowRecordFor(
  records: readonly AgentArtifactFollowRecord[],
  owner: AgentArtifactOwner | null | undefined,
): AgentArtifactFollowRecord | null {
  if (!validAgentArtifactOwner(owner)) return null;
  return (
    records.find((record) => sameAgentArtifactOwner(record.owner, owner)) ??
    null
  );
}

function withRecentTarget(
  recent: readonly AgentArtifactTarget[],
  target: AgentArtifactTarget,
): AgentArtifactTarget[] {
  return [
    ...recent.filter((item) => !sameAgentArtifactTarget(item, target)),
    target,
  ].slice(-MAX_AGENT_ARTIFACT_TARGETS);
}

function replaceOwnerRecord(
  records: readonly AgentArtifactFollowRecord[],
  next: AgentArtifactFollowRecord,
): AgentArtifactFollowRecord[] {
  return records
    .filter(
      (record) =>
        !sameAgentArtifactOwner(record.owner, next.owner) &&
        !(
          record.owner.projectId === next.owner.projectId &&
          record.owner.chatId === next.owner.chatId
        ),
    )
    .concat(next)
    .slice(-MAX_AGENT_ARTIFACT_FOLLOW_OWNERS);
}

function initialRecord(owner: AgentArtifactOwner): AgentArtifactFollowRecord {
  return {
    owner,
    mode: "follow",
    selected: null,
    recent: [],
    pending: [],
    lastPreviewRevision: 0,
  };
}

function validPreviewRevision(
  revision: number | undefined,
): revision is number {
  return Number.isSafeInteger(revision) && (revision ?? 0) > 0;
}

export const useAgentArtifactFollowStore = create<AgentArtifactFollowStore>(
  (set, get) => ({
    records: [],
    observe: (owner, target, previewRevision) => {
      if (!validAgentArtifactOwner(owner) || !validAgentArtifactTarget(target))
        return;
      if (target.kind === "preview" && !validPreviewRevision(previewRevision))
        return;
      const current =
        agentArtifactFollowRecordFor(get().records, owner) ??
        initialRecord(owner);
      if (
        target.kind === "preview" &&
        previewRevision! <= current.lastPreviewRevision
      ) {
        return;
      }
      const recent = withRecentTarget(current.recent, target);
      const pending =
        current.mode === "pinned" &&
        !sameAgentArtifactTarget(target, current.selected)
          ? withRecentTarget(current.pending, target)
          : current.pending;
      set({
        records: replaceOwnerRecord(get().records, {
          ...current,
          owner,
          selected: current.mode === "follow" ? target : current.selected,
          recent,
          pending,
          lastPreviewRevision:
            target.kind === "preview"
              ? previewRevision!
              : current.lastPreviewRevision,
        }),
      });
    },
    pin: (owner, target) => {
      if (!validAgentArtifactOwner(owner) || !validAgentArtifactTarget(target))
        return;
      const current =
        agentArtifactFollowRecordFor(get().records, owner) ??
        initialRecord(owner);
      set({
        records: replaceOwnerRecord(get().records, {
          ...current,
          owner,
          mode: "pinned",
          selected: target,
          recent: withRecentTarget(current.recent, target),
          pending: [],
        }),
      });
    },
    followLatest: (owner) => {
      if (!validAgentArtifactOwner(owner)) return;
      const current = agentArtifactFollowRecordFor(get().records, owner);
      if (!current) return;
      set({
        records: replaceOwnerRecord(get().records, {
          ...current,
          mode: "follow",
          selected: current.pending.at(-1) ?? current.recent.at(-1) ?? null,
          pending: [],
        }),
      });
    },
    reconcile: (owner, available) => {
      if (
        !validAgentArtifactOwner(owner) ||
        available.length > MAX_AGENT_ARTIFACT_TARGETS ||
        available.some((target) => !validAgentArtifactTarget(target))
      ) {
        return;
      }
      const current = agentArtifactFollowRecordFor(get().records, owner);
      if (!current) return;
      const retainedRecent = current.recent.filter((target) =>
        available.some((candidate) =>
          sameAgentArtifactTarget(candidate, target),
        ),
      );
      const pending = current.pending.filter((target) =>
        available.some((candidate) =>
          sameAgentArtifactTarget(candidate, target),
        ),
      );
      const selectedAvailable = available.some((target) =>
        sameAgentArtifactTarget(target, current.selected),
      );
      const recent =
        selectedAvailable &&
        validAgentArtifactTarget(current.selected) &&
        !retainedRecent.some((target) =>
          sameAgentArtifactTarget(target, current.selected),
        )
          ? withRecentTarget(retainedRecent, current.selected)
          : retainedRecent;
      if (recent.length === 0) {
        set({
          records: get().records.filter(
            (record) => !sameAgentArtifactOwner(record.owner, owner),
          ),
        });
        return;
      }
      const mode =
        current.mode === "pinned" && selectedAvailable ? "pinned" : "follow";
      const selected =
        mode === "pinned" ? current.selected : (recent.at(-1) ?? null);
      const samePending =
        pending.length === current.pending.length &&
        pending.every((target, index) =>
          sameAgentArtifactTarget(target, current.pending[index]),
        );
      if (
        mode === current.mode &&
        sameAgentArtifactTarget(selected, current.selected) &&
        recent.length === current.recent.length &&
        samePending
      ) {
        return;
      }
      set({
        records: replaceOwnerRecord(get().records, {
          ...current,
          mode,
          selected,
          recent,
          pending: mode === "pinned" ? pending : [],
        }),
      });
    },
    forgetTab: (tabId) => {
      if (!isShortId(tabId)) return;
      const records = get().records.flatMap((record) => {
        const recent = record.recent.filter(
          (target) => target.kind !== "tab" || target.tabId !== tabId,
        );
        const pending = record.pending.filter(
          (target) => target.kind !== "tab" || target.tabId !== tabId,
        );
        const selectedClosed =
          record.selected?.kind === "tab" && record.selected.tabId === tabId;
        if (!selectedClosed && recent.length === record.recent.length) {
          return [record];
        }
        if (recent.length === 0) return [];
        return [
          {
            ...record,
            mode: selectedClosed ? ("follow" as const) : record.mode,
            selected: selectedClosed
              ? (recent.at(-1) ?? null)
              : record.selected,
            recent,
            pending: selectedClosed ? [] : pending,
          },
        ];
      });
      set({ records });
    },
    clear: () => set({ records: [] }),
  }),
);
