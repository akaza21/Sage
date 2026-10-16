import { useLayoutEffect, useRef } from "react";

import { compareDecimalRevision } from "../../../app/acpProjectionMerge";
import {
  MAX_RETAINED_ITEMS,
  MAX_RETAINED_TURNS,
} from "../../../app/acpProjectionStore";
import type { AcpChatPage, AcpItem } from "../../../domain/acp";
import { isEditingFiles } from "./editingFiles";
import { itemOwnershipMatches } from "./agentEditSummary";
import {
  getPermissionStatus,
  getTurnStatusLabel,
  terminalToolStates,
  terminalTurnStates,
  toolStateLabels,
} from "./statusSemantics";

export interface Announcement {
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
      if (previousTurn === turn) continue;
      const newLatest = !previousTurn && isNewLatestTurn(previous, page, turn);
      if (!previousTurn && !newLatest) continue;
      const editing = isEditingFiles(page, turn);
      const wasEditing = previousTurn
        ? isEditingFiles(previous, previousTurn)
        : false;
      if (editing !== wasEditing) {
        polite.push({
          id: `editing:${turn.id}:${page.projectionRevision}`,
          text: editing ? "Editing files" : "Finished editing files",
        });
      }
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

function currentAnnouncements(
  page: AcpChatPage,
  resumed: boolean,
): Announcement[] {
  const turns = page.turns.filter(
    (turn) => turn.sessionId === page.currentSessionId,
  );
  const editing = turns.find((turn) => isEditingFiles(page, turn));
  const announcements: Announcement[] = [];
  if (
    editing ||
    (resumed &&
      turns.some((turn) =>
        turn.items.some(
          (item) =>
            item.kind === "fileChangeDiff" &&
            itemOwnershipMatches(page, turn, item),
        ),
      ))
  ) {
    announcements.push({
      id: `editing:current:${page.projectionRevision}`,
      text: editing ? "Editing files" : "Finished editing files",
    });
  }
  const terminal = resumed
    ? turns
        .filter((turn) => terminalTurnStates.has(turn.state))
        .sort((left, right) =>
          compareTurnOrder(turnOrder(page, right), turnOrder(page, left)),
        )[0]
    : undefined;
  if (terminal)
    announcements.push({
      id: `turn:${terminal.id}:${terminal.state}`,
      text: getTurnStatusLabel(terminal),
    });
  return announcements;
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
  if (!region) {
    if (drain.frame) cancelAnimationFrame(drain.frame);
    drain.frame = 0;
    if (drain.region) clearRegion(drain.region);
  }
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

export function AcpAnnouncementRegions({
  page,
  enabled = true,
  feedback,
  onFeedbackQueued,
}: {
  page: AcpChatPage | null;
  enabled?: boolean;
  feedback?: Announcement | null;
  onFeedbackQueued?: (id: string) => void;
}) {
  const previousRef = useRef<AcpChatPage | null>(null);
  const enabledRef = useRef(enabled);
  const politeRef = useRef<HTMLDivElement>(null);
  const assertiveRef = useRef<HTMLDivElement>(null);
  const politeDrainRef = useRef<AnnouncementDrain>(createDrain());
  const assertiveDrainRef = useRef<AnnouncementDrain>(createDrain());

  useLayoutEffect(() => {
    const previous = previousRef.current;
    previousRef.current = page;
    const resumed = enabled && !enabledRef.current;
    enabledRef.current = enabled;
    const scopeChanged =
      previous &&
      (!page ||
        previous.projectId !== page.projectId ||
        previous.chatId !== page.chatId ||
        (previous.currentSessionId !== null &&
          previous.currentSessionId !== page.currentSessionId));
    if (scopeChanged || !enabled || resumed) {
      resetDrain(politeDrainRef.current);
      resetDrain(assertiveDrainRef.current);
    }
    if (!enabled) return;
    const announcements =
      previous && page && !scopeChanged && !resumed
        ? deriveAnnouncements(previous, page)
        : {
            assertive: [],
            polite: page
              ? currentAnnouncements(page, resumed && !scopeChanged)
              : [],
          };
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
  }, [enabled, page]);

  useLayoutEffect(() => {
    if (feedback) {
      if (enabled)
        enqueueAnnouncements(politeDrainRef.current, politeRef.current, [
          feedback,
        ]);
      onFeedbackQueued?.(feedback.id);
    }
  }, [enabled, feedback, onFeedbackQueued]);

  useLayoutEffect(() => {
    const politeDrain = politeDrainRef.current;
    const assertiveDrain = assertiveDrainRef.current;
    return () => {
      previousRef.current = null;
      resetDrain(politeDrain);
      resetDrain(assertiveDrain);
    };
  }, []);

  return (
    <>
      <div
        ref={politeRef}
        className="sr-only"
        role={enabled ? "status" : undefined}
        aria-live={enabled ? "polite" : "off"}
        aria-atomic="true"
        data-acp-announcement="polite"
      />
      <div
        ref={assertiveRef}
        className="sr-only"
        role={enabled ? "status" : undefined}
        aria-live={enabled ? "assertive" : "off"}
        aria-atomic="true"
        data-acp-announcement="assertive"
      />
    </>
  );
}
