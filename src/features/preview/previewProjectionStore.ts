import { useEffect, useState } from "react";
import { createStore, useStore } from "zustand";

import type {
  PreviewProjection,
  PreviewProjectionChangedEvent,
  PreviewProjectionOwner,
} from "../../domain/previewProjection";
import { acpDesktop } from "../../services/acpDesktop";
import { previewOwnerKey, samePreviewOwner } from "./previewPresentation";

type PreviewState = {
  owner: PreviewProjectionOwner | null;
  projection: PreviewProjection | null;
  error: string | null;
  activate: (owner: PreviewProjectionOwner | null) => void;
  accept: (event: PreviewProjectionChangedEvent) => void;
  fail: (owner: PreviewProjectionOwner, error: unknown) => void;
};

type Sink = {
  event: (event: PreviewProjectionChangedEvent) => void;
  error: (error: unknown) => void;
  ready?: () => void;
};

type Subscribe = (
  onChange: (event: PreviewProjectionChangedEvent) => void,
) => Promise<() => void>;

const MAX_LISTENER_RETRIES = 1;

function transitionAllowed(
  previous: PreviewProjection["phase"],
  next: PreviewProjection["phase"],
): boolean {
  return (
    previous === next ||
    (previous === "idle" && (next === "discovering" || next === "starting")) ||
    (previous === "discovering" && next === "idle") ||
    (previous === "starting" &&
      ["waitingForOrigin", "healthFailed", "exited", "stopping"].includes(
        next,
      )) ||
    (previous === "waitingForOrigin" &&
      ["healthChecking", "healthFailed", "exited", "stopping"].includes(
        next,
      )) ||
    (previous === "healthChecking" &&
      ["active", "healthFailed", "exited", "stopping"].includes(next)) ||
    (previous === "active" &&
      ["healthChecking", "stopping", "exited"].includes(next)) ||
    (["healthFailed", "exited", "stopped"].includes(previous) &&
      ["discovering", "starting"].includes(next)) ||
    (previous === "stopping" && next === "stopped")
  );
}

function message(error: unknown): string {
  const value = error instanceof Error ? error.message : String(error);
  return value.slice(0, 8192) || "Preview updates are unavailable.";
}

export function createPreviewProjectionStore() {
  return createStore<PreviewState>((set, get) => ({
    owner: null,
    projection: null,
    error: null,
    activate: (owner) => {
      const current = get();
      if (
        samePreviewOwner(owner, current.owner) ||
        (!owner && !current.owner && !current.projection && !current.error)
      )
        return;
      set({ owner, projection: null, error: null });
    },
    accept: (event) => {
      const current = get();
      const next = event.preview;
      if (!samePreviewOwner(current.owner, next)) return;
      const previous = current.projection;
      if (
        previous &&
        (BigInt(next.previewRevision) <= BigInt(previous.previewRevision) ||
          !transitionAllowed(previous.phase, next.phase))
      ) {
        return;
      }
      set({ projection: next, error: null });
    },
    fail: (owner, error) => {
      if (samePreviewOwner(owner, get().owner)) set({ error: message(error) });
    },
  }));
}

export function createPreviewProjectionListener(subscribe: Subscribe) {
  let active: Sink | null = null;
  let pending: Promise<void> | null = null;
  let unlisten: (() => void) | null = null;
  let retries = 0;

  const ensure = () => {
    if (pending || unlisten) return;
    // Start through a promise so a synchronous loader failure follows the same
    // release path as an asynchronous Tauri listen failure.
    pending = Promise.resolve()
      .then(() => subscribe((event) => active?.event(event)))
      .then((dispose) => {
        pending = null;
        retries = 0;
        if (active) {
          unlisten = dispose;
          active.ready?.();
        } else dispose();
      })
      .catch((error) => {
        pending = null;
        active?.error(error);
        if (active && retries < MAX_LISTENER_RETRIES) {
          retries += 1;
          queueMicrotask(ensure);
        }
      });
  };

  return {
    acquire(sink: Sink): () => void {
      active = sink;
      if (unlisten) sink.ready?.();
      else ensure();
      return () => {
        if (active !== sink) return;
        active = null;
        if (unlisten) {
          unlisten();
          unlisten = null;
        }
      };
    },
  };
}

const listener = createPreviewProjectionListener(
  acpDesktop.subscribePreviewProjectionChanges,
);

export function usePreviewProjection(owner: PreviewProjectionOwner | null) {
  const [store] = useState(createPreviewProjectionStore);
  const [listenerReady, setListenerReady] = useState(false);
  const identity = previewOwnerKey(owner);
  // An owner switch is a rendering boundary, not an effect deferred until
  // after a replacement surface has painted.  Clearing projection and error
  // together prevents a new owner from observing the previous owner's origin.
  store.getState().activate(owner);
  useEffect(() => {
    if (!identity) return;
    return listener.acquire({
      event: (event) => store.getState().accept(event),
      error: (error) => {
        const activeOwner = store.getState().owner;
        if (activeOwner) store.getState().fail(activeOwner, error);
      },
      ready: () => setListenerReady(true),
    });
  }, [identity, store]);
  const state = useStore(store, (value) => value);
  return samePreviewOwner(owner, state.owner)
    ? { ...state, listenerReady }
    : { ...state, projection: null, error: null, listenerReady: false };
}
