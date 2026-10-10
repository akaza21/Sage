import { useLayoutEffect, useRef } from "react";

import { compareDecimalRevision } from "../../../app/acpProjectionMerge";
import {
  MAX_RETAINED_ITEMS,
  MAX_RETAINED_TURNS,
} from "../../../app/acpProjectionStore";
import type { AcpChatPage, AcpItem } from "../../../domain/acp";
import {
  getPermissionStatus,
  getTurnStatusLabel,
  terminalToolStates,
  terminalTurnStates,
  toolStateLabels,
} from "./statusSemantics";

interface Announcement {
  id: string;
  text: string;
}

interface AnnouncementSnapshot {
  assertive: Announcement[];
  polite: Announcement[];
}

function turnOrder(page: AcpChatPage, turn: AcpChatPage["turns"][number]) {
  return [
    page.sessions.find((session) => session.id === turn.sessionId)?.ordinal ??
      0,
    turn.ordinal,
    turn.createdAt,
    turn.id,
  ] as const;
}

function compareTurnOrder(
  left: ReturnType<typeof turnOrder>,
  right: ReturnType<typeof turnOrder>,
) {
  for (let index = 0; index < left.length; index += 1) {
    if (left[index] === right[index]) continue;
    return left[index] < right[index] ? -1 : 1;
  }
  return 0;
}

function isNewLatestTurn(
  previous: AcpChatPage,
  page: AcpChatPage,
  turn: AcpChatPage["turns"][number],
) {
  const currentOrder = turnOrder(page, turn);
  return previous.turns.every(
    (prior) => compareTurnOrder(currentOrder, turnOrder(previous, prior)) > 0,
  );
}

function terminalToolAnnouncement(item: AcpItem): string | null {
  if (item.kind !== "toolCall" || !terminalToolStates.has(item.toolState)) {
    return null;
  }
  return `${item.payload.presentation.label}: ${toolStateLabels[item.toolState]}`;
}

function deriveAnnouncements(
  previous: AcpChatPage,
  page: AcpChatPage,
): AnnouncementSnapshot {
  const polite: Announcement[] = [];
  const assertive: Announcement[] = [];
  const sameScope =
    previous.projectId === page.projectId && previous.chatId === page.chatId;
  const revisionAdvanced =
    compareDecimalRevision(
      page.projectionRevision,
      previous.projectionRevision,
    ) === 1;

  if (sameScope && revisionAdvanced) {
    const previousTurns = new Map(
      previous.turns.map((turn) => [turn.id, turn]),
    );
    for (const turn of page.turns) {
      const previousTurn = previousTurns.get(turn.id);
      const newLatest = !previousTurn && isNewLatestTurn(previous, page, turn);
      if (!previousTurn && !newLatest) continue;
      if (
        terminalTurnStates.has(turn.state) &&
        previousTurn?.state !== turn.state
      ) {
        polite.push({
          id: `turn:${turn.id}:${turn.state}`,
          text: getTurnStatusLabel(turn),
        });
      }
      for (const item of turn.items) {
        const prior = previousTurn?.items.find(
          (previousItem) => previousItem.id === item.id,
        );
        const toolAnnouncement = terminalToolAnnouncement(item);
        if (toolAnnouncement && prior?.toolState !== item.toolState) {
          polite.push({
            id: `tool:${item.id}:${item.toolState}`,
            text: toolAnnouncement,
          });
        }
        if (item.kind === "permissionRequest") {
          const currentStatus = getPermissionStatus(item);
          const priorStatus =
            prior?.kind === "permissionRequest"
              ? getPermissionStatus(prior)
              : null;
          if (
            currentStatus.terminal &&
            currentStatus.accessible !== priorStatus?.accessible
          ) {
            assertive.push({
              id: `permission:${item.id}:${currentStatus.accessible}`,
              text: currentStatus.accessible,
            });
          }
        }
      }
    }
  }
  return { assertive, polite };
}

function clearRegion(region: HTMLElement) {
  region.removeAttribute("aria-label");
  region.textContent = "";
}

interface AnnouncementDrain {
  currentId: string | null;
  frame: number;
  pending: Announcement[];
  pendingIds: Set<string>;
  region: HTMLElement | null;
}

const MAX_PENDING_ANNOUNCEMENTS = MAX_RETAINED_TURNS + MAX_RETAINED_ITEMS;

function createDrain(): AnnouncementDrain {
  return {
    currentId: null,
    frame: 0,
    pending: [],
    pendingIds: new Set(),
    region: null,
  };
}

function showNext(drain: AnnouncementDrain) {
  drain.frame = 0;
  const announcement = drain.pending.shift();
  const region = drain.region;
  if (!announcement || !region) return;
  drain.pendingIds.delete(announcement.id);
  drain.currentId = announcement.id;
  region.setAttribute("aria-label", announcement.text);
  region.textContent = announcement.text;
  if (drain.pending.length > 0) scheduleDrain(drain);
}

function scheduleDrain(drain: AnnouncementDrain) {
  const region = drain.region;
  if (drain.frame || !region || drain.pending.length === 0) return;
  if (drain.currentId) {
    drain.frame = requestAnimationFrame(() => {
      drain.frame = 0;
      clearRegion(region);
      drain.currentId = null;
      drain.frame = requestAnimationFrame(() => showNext(drain));
    });
    return;
  }
  drain.frame = requestAnimationFrame(() => showNext(drain));
}

function enqueueAnnouncements(
  drain: AnnouncementDrain,
  region: HTMLElement | null,
  announcements: readonly Announcement[],
) {
  drain.region = region;
  for (const announcement of announcements) {
    if (
      drain.pending.length >= MAX_PENDING_ANNOUNCEMENTS ||
      drain.currentId === announcement.id ||
      drain.pendingIds.has(announcement.id)
    ) {
      continue;
    }
    drain.pending.push(announcement);
    drain.pendingIds.add(announcement.id);
  }
  scheduleDrain(drain);
}

function resetDrain(drain: AnnouncementDrain) {
  if (drain.frame) cancelAnimationFrame(drain.frame);
  if (drain.region) clearRegion(drain.region);
  drain.currentId = null;
  drain.frame = 0;
  drain.pending = [];
  drain.pendingIds.clear();
  drain.region = null;
}

export function AcpAnnouncementRegions({ page }: { page: AcpChatPage }) {
  const previousRef = useRef<AcpChatPage | null>(null);
  const politeRef = useRef<HTMLDivElement>(null);
  const assertiveRef = useRef<HTMLDivElement>(null);
  const politeDrainRef = useRef<AnnouncementDrain>(createDrain());
  const assertiveDrainRef = useRef<AnnouncementDrain>(createDrain());

  useLayoutEffect(() => {
    const previous = previousRef.current;
    previousRef.current = page;
    if (
      previous &&
      (previous.projectId !== page.projectId || previous.chatId !== page.chatId)
    ) {
      resetDrain(politeDrainRef.current);
      resetDrain(assertiveDrainRef.current);
    }
    const announcements = previous
      ? deriveAnnouncements(previous, page)
      : { assertive: [], polite: [] };
    enqueueAnnouncements(
      politeDrainRef.current,
      politeRef.current,
      announcements.polite,
    );
    enqueueAnnouncements(
      assertiveDrainRef.current,
      assertiveRef.current,
      announcements.assertive,
    );
  }, [page]);

  useLayoutEffect(() => {
    const politeDrain = politeDrainRef.current;
    const assertiveDrain = assertiveDrainRef.current;
    return () => {
      resetDrain(politeDrain);
      resetDrain(assertiveDrain);
    };
  }, []);

  return (
    <>
      <div
        ref={politeRef}
        className="sr-only"
        role="status"
        aria-live="polite"
        aria-atomic="true"
        data-acp-announcement="polite"
      />
      <div
        ref={assertiveRef}
        className="sr-only"
        role="status"
        aria-live="assertive"
        aria-atomic="true"
        data-acp-announcement="assertive"
      />
    </>
  );
}
