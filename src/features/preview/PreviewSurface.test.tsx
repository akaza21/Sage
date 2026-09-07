import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const previewBridge = vi.hoisted(() => ({
  discover: vi.fn(),
  start: vi.fn(),
  stop: vi.fn(),
  subscribe: vi.fn(),
}));

vi.mock("./previewDesktop", () => ({
  discoverPreviewTargets: previewBridge.discover,
  startPreviewTarget: previewBridge.start,
  stopPreviewTarget: previewBridge.stop,
}));

vi.mock("../../services/acpDesktop", () => ({
  acpDesktop: { subscribePreviewProjectionChanges: previewBridge.subscribe },
}));

import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
import type {
  PreviewProjectionChangedEvent,
  PreviewProjectionOwner,
  PreviewTargetRef,
} from "../../domain/previewProjection";
import { browseSessionForOwner, setHarnessBrowseOwner } from "./harnessBrowse";
import { PreviewSurface } from "./PreviewSurface";

const initialState = useWorkbenchStore.getState();
const owner = {
  projectId: "project-a",
  projectGeneration: "7",
  transitionEpoch: 7,
  chatId: "chat-a",
  sessionId: "session-a",
  runtimeEpoch: "3",
} as PreviewProjectionOwner;
const replacementOwner = {
  ...owner,
  chatId: "chat-b",
  sessionId: "session-b",
  runtimeEpoch: "4",
} as PreviewProjectionOwner;
let receive: ((event: PreviewProjectionChangedEvent) => void) | null = null;
const unlisten = vi.fn();

function targetFor(value: PreviewProjectionOwner): PreviewTargetRef {
  return {
    projectId: value.projectId,
    projectGeneration: value.projectGeneration,
    expectedTransitionEpoch: value.transitionEpoch,
    chatId: value.chatId,
    sessionId: value.sessionId,
    runtimeEpoch: value.runtimeEpoch,
    rootId: `root-${value.chatId}`,
    scriptName: "dev",
  } as PreviewTargetRef;
}

function eventFor(
  value: PreviewProjectionOwner,
  revision: string,
  phase: string,
  overrides: Partial<PreviewProjectionChangedEvent["preview"]> = {},
): PreviewProjectionChangedEvent {
  return {
    kind: "preview",
    preview: {
      ...value,
      previewRevision: revision,
      phase,
      failure: null,
      target: null,
      origin: null,
      ...overrides,
    },
  } as PreviewProjectionChangedEvent;
}

function deliver(event: PreviewProjectionChangedEvent): void {
  act(() => receive?.(event));
}

type Deferred<Value> = {
  promise: Promise<Value>;
  resolve: (value: Value) => void;
  reject: (reason: Error) => void;
};

function deferred<Value>(): Deferred<Value> {
  let resolvePromise: ((value: Value) => void) | undefined;
  let rejectPromise: ((reason: Error) => void) | undefined;
  const promise = new Promise<Value>((resolve, reject) => {
    resolvePromise = resolve;
    rejectPromise = reject;
  });
  return {
    promise,
    resolve: (value) => {
      if (!resolvePromise)
        throw new Error("Deferred resolver was unavailable.");
      resolvePromise(value);
    },
    reject: (reason) => {
      if (!rejectPromise) throw new Error("Deferred rejecter was unavailable.");
      rejectPromise(reason);
    },
  };
}

function seedOwnedSession(value = owner): void {
  const store = acpProjectionStoreFor(value.chatId);
  const scope = {
    projectId: value.projectId,
    projectGeneration: value.projectGeneration,
    chatId: value.chatId,
  } as never;
  store.getState().activateScope(scope);
  store.getState().applyAuthoritativePage(scope, {
    projectId: value.projectId,
    chatId: value.chatId,
    title: "Chat A",
    projectionRevision: "1",
    currentSessionId: value.sessionId,
    sessions: [{ id: value.sessionId, runtimeEpoch: value.runtimeEpoch }],
    turns: [],
    nextCursor: null,
  } as never);
}

beforeEach(() => {
  previewBridge.discover.mockReset();
  previewBridge.start.mockReset();
  previewBridge.stop.mockReset();
  previewBridge.subscribe.mockReset();
  receive = null;
  unlisten.mockReset();
  previewBridge.discover.mockResolvedValue({ targets: [], truncated: false });
  previewBridge.start.mockResolvedValue(undefined);
  previewBridge.stop.mockResolvedValue(undefined);
  previewBridge.subscribe.mockImplementation(async (onChange) => {
    receive = onChange;
    return unlisten;
  });
  clearAcpProjectionRegistry();
  setHarnessBrowseOwner(null);
  useWorkbenchStore.setState({
    ...initialState,
    project: { id: "project-a", name: "A", roots: [] },
    nativeTransitionEpoch: 7,
    projectTransitioning: false,
  });
});

afterEach(() => {
  cleanup();
  setHarnessBrowseOwner(null);
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initialState, true);
});

describe("PreviewSurface", () => {
  it("keeps Preview inert without a fully owned live Chat session", () => {
    render(<PreviewSurface />);
    expect(screen.getByRole("region", { name: "Preview" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refresh" })).toBeDisabled();
  });

  it("sends the native discovery request shape without the event transitionEpoch field", async () => {
    seedOwnedSession();
    setHarnessBrowseOwner(owner, ["chat-a"]);
    render(<PreviewSurface />);

    await waitFor(() => expect(previewBridge.discover).toHaveBeenCalledOnce());
    expect(previewBridge.discover).toHaveBeenCalledWith({
      projectId: "project-a",
      projectGeneration: "7",
      expectedTransitionEpoch: 7,
      chatId: "chat-a",
      sessionId: "session-a",
      runtimeEpoch: "3",
    });
    expect(previewBridge.discover.mock.calls[0]?.[0]).not.toHaveProperty(
      "transitionEpoch",
    );
  });

  it("observes listener readiness before its first automatic discovery", async () => {
    let ready: ((dispose: () => void) => void) | null = null;
    previewBridge.subscribe.mockImplementation(
      () =>
        new Promise<() => void>((resolve) => {
          ready = resolve;
        }),
    );
    seedOwnedSession();
    setHarnessBrowseOwner(owner, ["chat-a"]);
    render(<PreviewSurface />);

    await Promise.resolve();
    expect(previewBridge.subscribe).toHaveBeenCalledOnce();
    expect(previewBridge.discover).not.toHaveBeenCalled();
    if (!ready) throw new Error("listener readiness resolver was unavailable");
    (ready as (dispose: () => void) => void)(unlisten);
    await waitFor(() => expect(previewBridge.discover).toHaveBeenCalledOnce());
  });

  it("navigates only the exact current owner's active native origin", async () => {
    seedOwnedSession(owner);
    seedOwnedSession(replacementOwner);
    setHarnessBrowseOwner(owner, ["chat-a"]);
    render(<PreviewSurface />);
    await waitFor(() => expect(receive).not.toBeNull());
    const target = targetFor(owner);
    deliver(
      eventFor(owner, "2", "active", {
        target,
        origin: "http://127.0.0.1:4173/a",
      }),
    );
    await waitFor(() =>
      expect(browseSessionForOwner(owner)?.href).toBe(
        "http://127.0.0.1:4173/a",
      ),
    );

    act(() => setHarnessBrowseOwner(replacementOwner, ["chat-b"]));
    deliver(
      eventFor(owner, "3", "active", {
        target,
        origin: "http://127.0.0.1:4173/stale",
      }),
    );
    expect(browseSessionForOwner(owner)).toBeNull();
    expect(browseSessionForOwner(replacementOwner)).toBeNull();

    deliver(
      eventFor(replacementOwner, "2", "active", {
        target: targetFor(replacementOwner),
        origin: "http://127.0.0.1:4173/b",
      }),
    );
    await waitFor(() =>
      expect(browseSessionForOwner(replacementOwner)?.href).toBe(
        "http://127.0.0.1:4173/b",
      ),
    );
  });

  it("renders native failure and keeps controls busy for a stopping projection", async () => {
    const target = targetFor(owner);
    previewBridge.discover.mockResolvedValue({
      targets: [target],
      truncated: false,
    });
    seedOwnedSession();
    setHarnessBrowseOwner(owner, ["chat-a"]);
    render(<PreviewSurface />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Start" })).toBeEnabled(),
    );
    deliver(eventFor(owner, "2", "healthFailed", { failure: "healthTimeout" }));
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Preview did not become healthy before its deadline.",
    );
    expect(screen.getByRole("button", { name: "Start" })).toBeEnabled();
    deliver(eventFor(owner, "3", "starting"));
    deliver(eventFor(owner, "4", "stopping", { target }));
    expect(screen.getByRole("button", { name: "Refresh" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Start" })).toBeDisabled();
  });

  it("fences stale discovery and Start failures after an owner transition", async () => {
    const staleDiscovery = deferred<{ targets: PreviewTargetRef[] }>();
    previewBridge.discover
      .mockReturnValueOnce(staleDiscovery.promise)
      .mockResolvedValue({ targets: [], truncated: false });
    seedOwnedSession(owner);
    seedOwnedSession(replacementOwner);
    setHarnessBrowseOwner(owner, ["chat-a"]);
    render(<PreviewSurface />);
    await waitFor(() => expect(previewBridge.discover).toHaveBeenCalledOnce());
    act(() => setHarnessBrowseOwner(replacementOwner, ["chat-b"]));
    staleDiscovery.resolve({ targets: [targetFor(owner)] });
    await waitFor(() =>
      expect(previewBridge.discover).toHaveBeenCalledTimes(2),
    );
    expect(screen.queryByText("dev")).toBeNull();

    const currentTarget = targetFor(replacementOwner);
    previewBridge.discover.mockResolvedValue({
      targets: [currentTarget],
      truncated: false,
    });
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Start" })).toBeEnabled(),
    );
    const staleStart = deferred<void>();
    previewBridge.start.mockReturnValueOnce(staleStart.promise);
    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    expect(previewBridge.start).toHaveBeenCalledWith(currentTarget);
    seedOwnedSession(owner);
    act(() => setHarnessBrowseOwner(owner, ["chat-a"]));
    staleStart.reject(new Error("stale start failure"));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  });

  it("dispatches Stop and cleanup only for the captured active target", async () => {
    seedOwnedSession(owner);
    seedOwnedSession(replacementOwner);
    setHarnessBrowseOwner(owner, ["chat-a"]);
    const target = targetFor(owner);
    previewBridge.discover.mockResolvedValue({
      targets: [target],
      truncated: false,
    });
    const view = render(<PreviewSurface />);
    await waitFor(() => expect(receive).not.toBeNull());
    deliver(
      eventFor(owner, "2", "active", {
        target,
        origin: "http://127.0.0.1:4173/a",
      }),
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled(),
    );
    const staleStop = deferred<void>();
    previewBridge.stop.mockReturnValueOnce(staleStop.promise);
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(previewBridge.stop).toHaveBeenCalledWith(target);

    act(() => setHarnessBrowseOwner(replacementOwner, ["chat-b"]));
    expect(previewBridge.stop).toHaveBeenCalledWith(target);
    expect(previewBridge.stop).toHaveBeenCalledTimes(2);
    staleStop.reject(new Error("stale stop failure"));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    view.unmount();
    expect(previewBridge.stop).toHaveBeenCalledTimes(2);
  });
});
