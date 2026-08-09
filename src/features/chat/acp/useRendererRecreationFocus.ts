import { useEffect, useRef, useState, type RefObject } from "react";

import { compareDecimalRevision } from "../../../app/acpProjectionMerge";
import type { AcpChatPage } from "../../../domain/acp";

const storageKey = "sage:renderer-recreation-focus:v1";
const maximumEnvelopeBytes = 4096;
const maximumIdentityBytes = 512;

interface FocusEnvelope {
  schemaVersion: 1;
  projectId: string;
  chatId: string;
  projectionRevision: string;
  sageId: string;
  activityId: string;
}

function utf8Bytes(value: string) {
  return new TextEncoder().encode(value).byteLength;
}

function readEnvelope(): FocusEnvelope | null {
  let raw: string | null;
  try {
    raw = sessionStorage.getItem(storageKey);
    sessionStorage.removeItem(storageKey);
  } catch {
    return null;
  }
  if (!raw || utf8Bytes(raw) > maximumEnvelopeBytes) return null;
  try {
    const value = JSON.parse(raw) as unknown;
    if (!value || typeof value !== "object" || Array.isArray(value))
      return null;
    const record = value as Record<string, unknown>;
    const expected = [
      "activityId",
      "chatId",
      "projectId",
      "projectionRevision",
      "sageId",
      "schemaVersion",
    ];
    if (Object.keys(record).sort().join("\0") !== expected.join("\0"))
      return null;
    if (
      record.schemaVersion !== 1 ||
      typeof record.projectId !== "string" ||
      typeof record.chatId !== "string" ||
      typeof record.sageId !== "string" ||
      typeof record.activityId !== "string" ||
      typeof record.projectionRevision !== "string" ||
      !/^[1-9][0-9]{0,19}$/.test(record.projectionRevision) ||
      [record.projectId, record.chatId, record.sageId, record.activityId].some(
        (field) =>
          field.length === 0 || utf8Bytes(field) > maximumIdentityBytes,
      )
    ) {
      return null;
    }
    return record as unknown as FocusEnvelope;
  } catch {
    return null;
  }
}

function writeEnvelope(surface: HTMLElement, page: AcpChatPage) {
  const active = document.activeElement;
  if (!(active instanceof HTMLElement) || !surface.contains(active)) return;
  const target = active.closest<HTMLElement>("[data-sage-id]");
  const activity = target?.closest<HTMLElement>("[data-sage-activity-id]");
  const sageId = target?.dataset.sageId;
  const activityId = activity?.dataset.sageActivityId;
  if (!sageId || !activityId) return;
  const envelope: FocusEnvelope = {
    schemaVersion: 1,
    projectId: page.projectId,
    chatId: page.chatId,
    projectionRevision: page.projectionRevision,
    sageId,
    activityId,
  };
  const raw = JSON.stringify(envelope);
  if (utf8Bytes(raw) > maximumEnvelopeBytes) return;
  try {
    sessionStorage.setItem(storageKey, raw);
  } catch {
    // Renderer recreation focus is a best-effort disposable enhancement.
  }
}

function matchingElement(
  root: HTMLElement,
  attribute: "sageId" | "sageActivityId",
  value: string,
) {
  const selector =
    attribute === "sageId" ? "[data-sage-id]" : "[data-sage-activity-id]";
  return [...root.querySelectorAll<HTMLElement>(selector)].find(
    (element) => element.dataset[attribute] === value,
  );
}

export function useRendererRecreationFocus(
  surfaceRef: RefObject<HTMLElement | null>,
  page: AcpChatPage | null,
) {
  const [pending] = useState(readEnvelope);
  const consumedRef = useRef(false);
  const pageRef = useRef(page);

  useEffect(() => {
    pageRef.current = page;
  }, [page]);

  useEffect(() => {
    function preserveFocus() {
      const surface = surfaceRef.current;
      const currentPage = pageRef.current;
      if (surface && currentPage) writeEnvelope(surface, currentPage);
    }
    window.addEventListener("pagehide", preserveFocus);
    return () => window.removeEventListener("pagehide", preserveFocus);
  }, [surfaceRef]);

  useEffect(() => {
    if (!pending || consumedRef.current || !page || !surfaceRef.current) return;
    if (
      pending.projectId !== page.projectId ||
      pending.chatId !== page.chatId
    ) {
      consumedRef.current = true;
      return;
    }
    const revision = compareDecimalRevision(
      page.projectionRevision,
      pending.projectionRevision,
    );
    if (revision === null) {
      consumedRef.current = true;
      return;
    }
    if (revision < 0) return;
    consumedRef.current = true;

    const surface = surfaceRef.current;
    const target = matchingElement(surface, "sageId", pending.sageId);
    if (target && !target.closest("details:not([open])")) {
      target.focus();
      if (document.activeElement === target) return;
    }
    const activity = matchingElement(
      surface,
      "sageActivityId",
      pending.activityId,
    );
    const summary = [...(activity?.children ?? [])].find(
      (element): element is HTMLElement =>
        element instanceof HTMLElement && element.tagName === "SUMMARY",
    );
    summary?.focus();
  }, [page, pending, surfaceRef]);
}
