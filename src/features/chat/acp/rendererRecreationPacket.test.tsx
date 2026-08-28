import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { useRef } from "react";

import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../../../app/acpProjectionRegistry";
import type { AcpChatPage } from "../../../domain/acp";
import { AcpAnnouncementRegions } from "./AcpAnnouncementRegions";
import {
  admitRendererRecreationPacket,
  collectRendererRecreationPacket,
  installRendererRecreationPacketCollector,
  mountRendererRecreationPacketCollector,
  RENDERER_RECREATION_PACKET_EVAL_SCRIPT,
} from "./rendererRecreationPacket";
import {
  resetRendererRecreationFocusEvidence,
  useRendererRecreationFocus,
} from "./useRendererRecreationFocus";

function page(revision = "2"): AcpChatPage {
  return {
    projectId: "project-a",
    chatId: "chat-a",
    title: "Durable chat",
    projectionRevision: revision,
    currentSessionId: null,
    sessions: [],
    turns: [],
    nextCursor: null,
  } as unknown as AcpChatPage;
}

function FocusHarness({
  value,
  open = true,
  target = true,
}: {
  value: AcpChatPage | null;
  open?: boolean;
  target?: boolean;
}) {
  const surfaceRef = useRef<HTMLElement>(null);
  useRendererRecreationFocus(surfaceRef, value);
  return (
    <section ref={surfaceRef}>
      <details data-sage-activity-id="activity-a" open={open}>
        <summary>Activity summary</summary>
        {target ? (
          <div data-sage-id="item-a" role="treeitem" tabIndex={0}>
            Item
          </div>
        ) : null}
      </details>
    </section>
  );
}

function mountedPacketHarness(
  announcementPages: AcpChatPage[],
  focusPage: AcpChatPage | null,
  focusProps?: { open?: boolean; target?: boolean },
) {
  acpProjectionStoreFor("chat-a");
  acpProjectionStoreFor("chat-b");
  return render(
    <>
      {announcementPages.map((value, index) => (
        <AcpAnnouncementRegions key={index} page={value} />
      ))}
      <FocusHarness value={focusPage} {...focusProps} />
    </>,
  );
}

function saveFocusedTarget(unmount: () => void) {
  const target = document.querySelector<HTMLElement>(
    '[data-sage-id="item-a"]',
  )!;
  target.focus();
  window.dispatchEvent(new Event("pagehide"));
  unmount();
}

afterEach(() => {
  cleanup();
  clearAcpProjectionRegistry();
  resetRendererRecreationFocusEvidence();
  sessionStorage.clear();
  Reflect.deleteProperty(window, "__SAGE_COLLECT_RENDERER_RECREATION_PACKET__");
});

describe("renderer recreation frontend packet", () => {
  it("admits two announcement nodes, one pagehide, and two occupied projection slots", () => {
    acpProjectionStoreFor("chat-a");
    acpProjectionStoreFor("chat-b");
    render(
      <>
        <AcpAnnouncementRegions page={page()} />
        <FocusHarness value={page()} />
      </>,
    );

    expect(
      admitRendererRecreationPacket(collectRendererRecreationPacket()),
    ).toEqual({
      schemaVersion: 1,
      liveRegions: { polite: 1, assertive: 1 },
      pagehideListeners: 1,
      focusOutcome: "no-restoration-when-no-valid-envelope",
      projectionSlots: 2,
    });
  });

  it("unmount admits empty packet; remount rebuilds the announcement pair", () => {
    const host = mountedPacketHarness([page()], page());

    expect(
      admitRendererRecreationPacket(collectRendererRecreationPacket()),
    ).toEqual({
      schemaVersion: 1,
      liveRegions: { polite: 1, assertive: 1 },
      pagehideListeners: 1,
      focusOutcome: "no-restoration-when-no-valid-envelope",
      projectionSlots: 2,
    });

    host.unmount();

    const destroyed = admitRendererRecreationPacket(
      collectRendererRecreationPacket(),
    );
    expect(destroyed).toEqual({
      schemaVersion: 1,
      liveRegions: { polite: 0, assertive: 0 },
      pagehideListeners: 0,
      focusOutcome: "no-restoration-when-no-valid-envelope",
      projectionSlots: 2,
    });

    mountedPacketHarness([page()], page());

    expect(
      admitRendererRecreationPacket(collectRendererRecreationPacket()),
    ).toEqual({
      schemaVersion: 1,
      liveRegions: { polite: 1, assertive: 1 },
      pagehideListeners: 1,
      focusOutcome: "no-restoration-when-no-valid-envelope",
      projectionSlots: 2,
    });
  });

  it("fails closed when four announcement nodes are present", () => {
    mountedPacketHarness([page(), page()], page());
    expect(collectRendererRecreationPacket()).toBeNull();
    expect(admitRendererRecreationPacket(null)).toBeNull();
  });

  it("fails closed when two pagehide listeners are registered", () => {
    acpProjectionStoreFor("chat-a");
    render(
      <>
        <AcpAnnouncementRegions page={page()} />
        <FocusHarness value={page()} />
        <FocusHarness value={page("3")} />
      </>,
    );
    expect(collectRendererRecreationPacket()).toBeNull();
  });

  it("fails closed when announcement nodes are present without a pagehide listener", () => {
    render(<AcpAnnouncementRegions page={page()} />);
    expect(collectRendererRecreationPacket()).toBeNull();
  });

  it("fails closed when a pagehide listener is present without announcement nodes", () => {
    render(<FocusHarness value={page()} />);
    expect(collectRendererRecreationPacket()).toBeNull();
  });

  it("records no-restoration-when-no-valid-envelope without an envelope", () => {
    mountedPacketHarness([page()], page());
    expect(collectRendererRecreationPacket()?.focusOutcome).toBe(
      "no-restoration-when-no-valid-envelope",
    );
  });

  it("records exact-sage-id after a successful restore", () => {
    const first = render(<FocusHarness value={page()} />);
    saveFocusedTarget(first.unmount);
    mountedPacketHarness([page()], page());
    expect(collectRendererRecreationPacket()?.focusOutcome).toBe(
      "exact-sage-id",
    );
  });

  it("records activity-summary-fallback when the exact target is hidden", () => {
    const first = render(<FocusHarness value={page()} />);
    saveFocusedTarget(first.unmount);
    mountedPacketHarness([page()], page(), { open: false, target: false });
    expect(collectRendererRecreationPacket()?.focusOutcome).toBe(
      "activity-summary-fallback",
    );
  });

  it("observer eval script returns null until the window collector is installed", () => {
    mountedPacketHarness([page()], page());
    expect(
      Reflect.get(window, "__SAGE_COLLECT_RENDERER_RECREATION_PACKET__"),
    ).toBeUndefined();
    expect((0, eval)(RENDERER_RECREATION_PACKET_EVAL_SCRIPT)).toBeNull();
  });

  it("observer eval script returns the admitted packet after collector install", () => {
    mountedPacketHarness([page()], page());
    installRendererRecreationPacketCollector();
    expect((0, eval)(RENDERER_RECREATION_PACKET_EVAL_SCRIPT)).toEqual(
      admitRendererRecreationPacket(collectRendererRecreationPacket()),
    );
  });

  it("installed collector still returns the empty packet after the renderer unmounts", () => {
    const host = mountedPacketHarness([page()], page());
    installRendererRecreationPacketCollector();
    host.unmount();
    expect((0, eval)(RENDERER_RECREATION_PACKET_EVAL_SCRIPT)).toEqual({
      schemaVersion: 1,
      liveRegions: { polite: 0, assertive: 0 },
      pagehideListeners: 0,
      focusOutcome: "no-restoration-when-no-valid-envelope",
      projectionSlots: 2,
    });
  });

  it("exposes the collector only while an admitted ACP page owns it", () => {
    mountedPacketHarness([page()], page());
    const release = mountRendererRecreationPacketCollector();

    expect((0, eval)(RENDERER_RECREATION_PACKET_EVAL_SCRIPT)).toEqual(
      admitRendererRecreationPacket(collectRendererRecreationPacket()),
    );

    release();
    expect((0, eval)(RENDERER_RECREATION_PACKET_EVAL_SCRIPT)).toBeNull();
  });
});
