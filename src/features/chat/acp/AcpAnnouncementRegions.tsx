import { useLayoutEffect, useRef, useState } from "react";

import { compareDecimalRevision } from "../../../app/acpProjectionMerge";
import {
  MAX_RETAINED_ITEMS,
  MAX_RETAINED_TURNS,
} from "../../../app/acpProjectionStore";
import type { AcpChatPage } from "../../../domain/acp";
import { isEditingFiles } from "./editingFiles";
import { itemOwnershipMatches } from "./agentEditSummary";
import {
  getPermissionStatus,
  getTurnStatusLabel,
  terminalToolStates,
  terminalTurnStates,
  turnReasonMessage,
  toolStateLabels,
} from "./statusSemantics";

export interface Announcement {
  id: string;
  text: string;
}

type Announce = (
  announcement: Announcement,
  priority?: "polite" | "assertive",
) => void;

function sessionOrdinal(page: AcpChatPage, turn: AcpChatPage["turns"][number]) {
  return (
    page.sessions.find((session) => session.id === turn.sessionId)?.ordinal ?? 0
  );
}

function isNewLatestTurn(
  previous: AcpChatPage,
  page: AcpChatPage,
  turn: AcpChatPage["turns"][number],
) {
  // Projection pages are chronological; native session/turn ordinals are unique.
  const prior = previous.turns.at(-1);
  if (!prior) return true;
  const session = sessionOrdinal(page, turn) - sessionOrdinal(previous, prior);
  return session > 0 || (session === 0 && turn.ordinal > prior.ordinal);
}

function deriveAnnouncements(
  previous: AcpChatPage,
  page: AcpChatPage,
  announce: Announce,
) {
  const revisionAdvanced =
    compareDecimalRevision(
      page.projectionRevision,
      previous.projectionRevision,
    ) === 1;

  if (revisionAdvanced) {
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
        announce({
          id: `editing:${turn.id}:${page.projectionRevision}`,
          text: editing ? "Editing files" : "Finished editing files",
        });
      }
      if (
        terminalTurnStates.has(turn.state) &&
        previousTurn?.state !== turn.state
      ) {
        const announcement = {
          id: `turn:${turn.id}:${turn.state}`,
          text:
            (turn.state === "failed" && turnReasonMessage(turn)) ||
            getTurnStatusLabel(turn),
        };
        announce(
          announcement,
          turn.state === "failed" ? "assertive" : "polite",
        );
      }
      for (const item of turn.items) {
        const prior = previousTurn?.items.find(
          (previousItem) => previousItem.id === item.id,
        );
        if (
          item.kind === "toolCall" &&
          terminalToolStates.has(item.toolState) &&
          prior?.toolState !== item.toolState
        ) {
          announce({
            id: `tool:${item.id}:${item.toolState}`,
            text: `${item.payload.presentation.label}: ${toolStateLabels[item.toolState]}`,
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
            announce(
              {
                id: `permission:${item.id}:${currentStatus.accessible}`,
                text: currentStatus.accessible,
              },
              "assertive",
            );
          }
        }
      }
    }
  }
}

function clearRegion(region: HTMLElement) {
  region.removeAttribute("aria-label");
  region.textContent = "";
}

function currentAnnouncements(
  page: AcpChatPage,
  resumed: boolean,
  announce: Announce,
) {
  const turns = page.turns.filter(
    (turn) => turn.sessionId === page.currentSessionId,
  );
  const editing = turns.find((turn) => isEditingFiles(page, turn));
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
    announce({
      id: `editing:current:${page.projectionRevision}`,
      text: editing ? "Editing files" : "Finished editing files",
    });
  }
  const terminal = resumed
    ? turns.filter((turn) => terminalTurnStates.has(turn.state)).at(-1)
    : undefined;
  if (terminal)
    announce({
      id: `turn:${terminal.id}:${terminal.state}`,
      text: getTurnStatusLabel(terminal),
    });
}

interface AnnouncementDrain {
  currentId: string | null;
  frame: number;
  pending: Map<string, string>;
  ref: { current: HTMLDivElement | null };
}

const MAX_PENDING_ANNOUNCEMENTS = MAX_RETAINED_TURNS + MAX_RETAINED_ITEMS;

function createDrain(): AnnouncementDrain {
  return {
    currentId: null,
    frame: 0,
    pending: new Map(),
    ref: { current: null },
  };
}

function scheduleDrain(drain: AnnouncementDrain) {
  const region = drain.ref.current;
  if (drain.frame || !region || drain.pending.size === 0) return;
  drain.frame = requestAnimationFrame(() => {
    drain.frame = 0;
    if (drain.currentId) {
      clearRegion(region);
      drain.currentId = null;
    } else {
      const [id, text] = drain.pending.entries().next().value!;
      drain.pending.delete(id);
      drain.currentId = id;
      region.setAttribute("aria-label", text);
      region.textContent = text;
    }
    scheduleDrain(drain);
  });
}

function enqueueAnnouncement(
  drain: AnnouncementDrain,
  announcement: Announcement,
) {
  if (
    drain.pending.size >= MAX_PENDING_ANNOUNCEMENTS ||
    drain.currentId === announcement.id ||
    drain.pending.has(announcement.id)
  ) {
    return;
  }
  drain.pending.set(announcement.id, announcement.text);
  scheduleDrain(drain);
}

function resetDrain(drain: AnnouncementDrain) {
  if (drain.frame) cancelAnimationFrame(drain.frame);
  if (drain.ref.current) clearRegion(drain.ref.current);
  drain.currentId = null;
  drain.frame = 0;
  drain.pending.clear();
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
  const announcerRef = useRef({
    previous: null as AcpChatPage | null,
    enabled,
  });
  const [drains] = useState(() => ({
    polite: createDrain(),
    assertive: createDrain(),
  }));

  useLayoutEffect(() => {
    const announcer = announcerRef.current;
    const previous = announcer.previous;
    announcer.previous = page;
    const resumed = enabled && !announcer.enabled;
    announcer.enabled = enabled;
    const scopeChanged =
      previous &&
      (!page ||
        previous.projectId !== page.projectId ||
        previous.chatId !== page.chatId ||
        (previous.currentSessionId !== null &&
          previous.currentSessionId !== page.currentSessionId));
    if (scopeChanged || !enabled || resumed) {
      Object.values(drains).forEach(resetDrain);
    }
    if (!enabled) return;
    const announce: Announce = (announcement, priority = "polite") =>
      enqueueAnnouncement(drains[priority], announcement);
    if (previous && page && !scopeChanged && !resumed)
      deriveAnnouncements(previous, page, announce);
    else if (page)
      currentAnnouncements(page, resumed && !scopeChanged, announce);
  }, [enabled, page, drains]);

  useLayoutEffect(() => {
    if (feedback) {
      if (enabled) enqueueAnnouncement(drains.polite, feedback);
      onFeedbackQueued?.(feedback.id);
    }
  }, [enabled, feedback, onFeedbackQueued, drains]);

  useLayoutEffect(() => {
    const announcer = announcerRef.current;
    return () => {
      announcer.previous = null;
      Object.values(drains).forEach(resetDrain);
    };
  }, [drains]);

  return (["polite", "assertive"] as const).map((level) => (
    <div
      key={level}
      ref={drains[level].ref}
      className="sr-only"
      role={enabled ? "status" : undefined}
      aria-live={enabled ? level : "off"}
      aria-atomic="true"
      data-acp-announcement={level}
    />
  ));
}
