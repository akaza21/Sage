import {
  MAX_VISIBLE_ACP_PROJECTION_SLOTS,
  occupiedAcpProjectionSlotCount,
} from "../../../app/acpProjectionRegistry";
import {
  RENDERER_RECREATION_FOCUS_OUTCOMES,
  lastRendererRecreationFocusOutcome,
  rendererRecreationPagehideListenerCount,
  type RendererRecreationFocusOutcome,
} from "./useRendererRecreationFocus";

export const RENDERER_RECREATION_PACKET_SCHEMA_VERSION = 1;
export const RENDERER_RECREATION_PACKET_EVAL_SCRIPT =
  '(function(){try{var collect=window.__SAGE_COLLECT_RENDERER_RECREATION_PACKET__;if(typeof collect!=="function")return null;return collect();}catch(_error){return null;}})()';
const MAX_LIVE_REGION_WALK = 8;
const MAX_ANNOUNCEMENT_KIND_BYTES = 16;
const MAX_PACKET_BYTES = 512;

export type RendererRecreationFrontendPacket = {
  schemaVersion: 1;
  liveRegions: {
    polite: 0 | 1;
    assertive: 0 | 1;
  };
  pagehideListeners: 0 | 1;
  focusOutcome: RendererRecreationFocusOutcome;
  projectionSlots: 0 | 1 | 2;
};

type RendererRecreationPacketCollectorHost = Window & {
  __SAGE_COLLECT_RENDERER_RECREATION_PACKET__?: () => RendererRecreationFrontendPacket | null;
};

function walkAnnouncementRegions(root: ParentNode): HTMLElement[] | null {
  if (typeof document === "undefined" || !("createTreeWalker" in document)) {
    return null;
  }
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT, {
    acceptNode(node) {
      return node instanceof HTMLElement &&
        node.hasAttribute("data-acp-announcement")
        ? NodeFilter.FILTER_ACCEPT
        : NodeFilter.FILTER_SKIP;
    },
  });
  const matches: HTMLElement[] = [];
  let current = walker.nextNode();
  while (current) {
    if (!(current instanceof HTMLElement)) return null;
    matches.push(current);
    if (matches.length === MAX_LIVE_REGION_WALK) {
      if (walker.nextNode()) return null;
      break;
    }
    current = walker.nextNode();
  }
  return matches;
}

function countLiveRegions(
  nodes: readonly HTMLElement[],
): RendererRecreationFrontendPacket["liveRegions"] | null {
  let polite = 0;
  let assertive = 0;
  for (const node of nodes) {
    const kind = node.dataset.acpAnnouncement ?? "";
    if (utf8ByteLength(kind) > MAX_ANNOUNCEMENT_KIND_BYTES) return null;
    if (kind === "polite") polite += 1;
    else if (kind === "assertive") assertive += 1;
    else return null;
  }
  if (polite + assertive > 2) return null;
  if (!(
    (polite === 1 && assertive === 1) ||
    (polite === 0 && assertive === 0)
  )) {
    return null;
  }
  return {
    polite,
    assertive,
  } as RendererRecreationFrontendPacket["liveRegions"];
}

function isFocusOutcome(
  value: string | null,
): value is RendererRecreationFocusOutcome {
  return (
    value !== null &&
    (RENDERER_RECREATION_FOCUS_OUTCOMES as readonly string[]).includes(value)
  );
}

export function installRendererRecreationPacketCollector(): void {
  if (typeof window === "undefined") return;
  (
    window as RendererRecreationPacketCollectorHost
  ).__SAGE_COLLECT_RENDERER_RECREATION_PACKET__ =
    collectRendererRecreationPacket;
}

export function mountRendererRecreationPacketCollector(): () => void {
  if (typeof window === "undefined") return () => undefined;
  const host = window as RendererRecreationPacketCollectorHost;
  host.__SAGE_COLLECT_RENDERER_RECREATION_PACKET__ =
    collectRendererRecreationPacket;
  return () => {
    if (
      host.__SAGE_COLLECT_RENDERER_RECREATION_PACKET__ ===
      collectRendererRecreationPacket
    ) {
      Reflect.deleteProperty(
        host,
        "__SAGE_COLLECT_RENDERER_RECREATION_PACKET__",
      );
    }
  };
}

export function collectRendererRecreationPacket(): RendererRecreationFrontendPacket | null {
  const nodes = walkAnnouncementRegions(document);
  if (!nodes) return null;
  const liveRegions = countLiveRegions(nodes);
  if (!liveRegions) return null;
  const pagehideListeners = rendererRecreationPagehideListenerCount();
  if (pagehideListeners !== 0 && pagehideListeners !== 1) return null;
  const projectionSlots = occupiedAcpProjectionSlotCount();
  if (
    projectionSlots < 0 ||
    projectionSlots > MAX_VISIBLE_ACP_PROJECTION_SLOTS
  ) {
    return null;
  }
  const focusOutcome = lastRendererRecreationFocusOutcome();
  if (!isFocusOutcome(focusOutcome)) return null;
  return admitRendererRecreationPacket({
    schemaVersion: RENDERER_RECREATION_PACKET_SCHEMA_VERSION,
    liveRegions,
    pagehideListeners,
    focusOutcome,
    projectionSlots: projectionSlots as 0 | 1 | 2,
  });
}

export function admitRendererRecreationPacket(
  packet: RendererRecreationFrontendPacket | null,
): RendererRecreationFrontendPacket | null {
  if (!packet) return null;
  const raw = JSON.stringify(packet);
  if (utf8ByteLength(raw) > MAX_PACKET_BYTES) return null;
  if (packet.schemaVersion !== RENDERER_RECREATION_PACKET_SCHEMA_VERSION) {
    return null;
  }
  if (!isFocusOutcome(packet.focusOutcome)) return null;
  if (utf8ByteLength(packet.focusOutcome) > MAX_PACKET_BYTES) return null;
  const { polite, assertive } = packet.liveRegions;
  if (!(
    (polite === 1 && assertive === 1) ||
    (polite === 0 && assertive === 0)
  )) {
    return null;
  }
  if (packet.pagehideListeners !== 0 && packet.pagehideListeners !== 1) {
    return null;
  }
  if (
    packet.projectionSlots < 0 ||
    packet.projectionSlots > MAX_VISIBLE_ACP_PROJECTION_SLOTS
  ) {
    return null;
  }
  if (polite === 1 && packet.pagehideListeners !== 1) return null;
  if (polite === 0 && packet.pagehideListeners !== 0) return null;
  return packet;
}
import { utf8ByteLength } from "../../../domain/text";
