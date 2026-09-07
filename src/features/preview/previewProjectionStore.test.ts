import { StrictMode, createElement } from "react";
import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const subscribePreviewChanges = vi.hoisted(() => vi.fn());

vi.mock("../../services/acpDesktop", () => ({
  acpDesktop: { subscribePreviewProjectionChanges: subscribePreviewChanges },
}));

import type {
  PreviewProjectionChangedEvent,
  PreviewProjectionOwner,
} from "../../domain/previewProjection";
import {
  createPreviewProjectionListener,
  createPreviewProjectionStore,
  usePreviewProjection,
} from "./previewProjectionStore";

const owner = {
  projectId: "project-a",
  projectGeneration: "7",
  transitionEpoch: 7,
  chatId: "chat-a",
  sessionId: "session-a",
  runtimeEpoch: "3",
} as PreviewProjectionOwner;

function event(
  revision: string,
  phase = "discovering",
): PreviewProjectionChangedEvent {
  return {
    kind: "preview",
    preview: {
      ...owner,
      previewRevision: revision,
      phase,
      failure: null,
      target: null,
      origin: null,
    },
  } as never;
}

async function settleListener(): Promise<void> {
  for (let index = 0; index < 8; index += 1) await Promise.resolve();
}

afterEach(() => {
  cleanup();
  subscribePreviewChanges.mockReset();
});

describe("Preview projection store", () => {
  it("does not publish repeatedly when an unavailable controller rerenders", () => {
    const store = createPreviewProjectionStore();
    const observe = vi.fn();
    const release = store.subscribe(observe);
    store.getState().activate(null);
    store.getState().activate(null);
    expect(observe).not.toHaveBeenCalled();
    release();
  });

  it("accepts a noncontiguous first revision, updates an equal phase, and rejects stale or illegal transitions", () => {
    const store = createPreviewProjectionStore();
    store.getState().activate(owner);
    store.getState().accept(event("2"));
    store.getState().accept(event("2", "idle"));
    store.getState().accept(event("1", "idle"));
    store.getState().accept(event("4", "idle"));
    store.getState().accept({
      ...event("5", "starting"),
      preview: { ...event("5", "starting").preview, failure: "conflict" },
    });
    store.getState().accept({
      ...event("6", "starting"),
      preview: { ...event("6", "starting").preview, failure: "internal" },
    });
    store.getState().accept(event("7", "active"));
    store.getState().accept({
      ...event("8"),
      preview: { ...event("8").preview, chatId: "other" as never },
    });
    expect(store.getState().projection).toMatchObject({
      previewRevision: "6",
      phase: "starting",
      failure: "internal",
    });
    store.getState().activate({ ...owner, sessionId: "session-b" } as never);
    expect(store.getState()).toMatchObject({ projection: null, error: null });
  });

  it("delivers a deferred registration only to the remounted survivor", async () => {
    const deferred: { resolve: ((dispose: () => void) => void) | null } = {
      resolve: null,
    };
    const dispose = vi.fn();
    const receiver: {
      current: ((event: PreviewProjectionChangedEvent) => void) | null;
    } = { current: null };
    const subscribe = vi.fn(
      (onChange: (event: PreviewProjectionChangedEvent) => void) =>
        new Promise<() => void>((done) => {
          receiver.current = onChange;
          deferred.resolve = done;
        }),
    );
    const listener = createPreviewProjectionListener(subscribe);
    const former = vi.fn();
    const survivor = vi.fn();
    const releaseFormer = listener.acquire({ event: former, error: vi.fn() });
    releaseFormer();
    const releaseSurvivor = listener.acquire({
      event: survivor,
      error: vi.fn(),
    });
    await settleListener();
    deferred.resolve?.(dispose);
    await settleListener();
    expect(subscribe).toHaveBeenCalledOnce();
    receiver.current?.(event("2"));
    expect(former).not.toHaveBeenCalled();
    expect(survivor).toHaveBeenCalledWith(event("2"));
    releaseSurvivor();
    expect(dispose).toHaveBeenCalledOnce();
  });

  it("retries a rejected import/listen slot while the original mount remains active", async () => {
    const rejected = new Error("event import failed");
    const firstError = vi.fn();
    const dispose = vi.fn();
    const subscribe = vi
      .fn()
      .mockRejectedValueOnce(rejected)
      .mockResolvedValueOnce(dispose);
    const listener = createPreviewProjectionListener(subscribe);
    const release = listener.acquire({ event: vi.fn(), error: firstError });
    await settleListener();
    expect(firstError).toHaveBeenCalledWith(rejected);
    expect(subscribe).toHaveBeenCalledTimes(2);
    await settleListener();
    release();
    release();
    expect(dispose).toHaveBeenCalledOnce();
  });

  it("replaces the sink without adding a listener and final disposal is idempotent", async () => {
    const dispose = vi.fn();
    const subscribe = vi.fn().mockResolvedValue(dispose);
    const listener = createPreviewProjectionListener(subscribe);
    const first = listener.acquire({ event: vi.fn(), error: vi.fn() });
    await settleListener();
    const second = listener.acquire({ event: vi.fn(), error: vi.fn() });
    first();
    expect(dispose).not.toHaveBeenCalled();
    second();
    second();
    expect(subscribe).toHaveBeenCalledOnce();
    expect(dispose).toHaveBeenCalledOnce();
  });

  it("reuses a semantically identical owner through StrictMode remounts", async () => {
    const dispose = vi.fn();
    subscribePreviewChanges.mockResolvedValue(dispose);
    function Probe({ value }: { value: PreviewProjectionOwner }) {
      usePreviewProjection(value);
      return null;
    }
    const view = render(
      createElement(StrictMode, null, createElement(Probe, { value: owner })),
    );
    await waitFor(() => expect(subscribePreviewChanges).toHaveBeenCalledOnce());
    view.rerender(
      createElement(
        StrictMode,
        null,
        createElement(Probe, { value: { ...owner } }),
      ),
    );
    await settleListener();
    expect(subscribePreviewChanges).toHaveBeenCalledOnce();
    view.unmount();
    expect(dispose).toHaveBeenCalledOnce();
  });
});
