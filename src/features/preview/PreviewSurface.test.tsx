import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import {
  browseSessionForOwner,
  clearBrowseSessions,
  setHarnessBrowseOwner,
} from "./harnessBrowse";
import { PreviewSurface } from "./PreviewSurface";
import type { PreviewHealthResponse, PreviewTargetRef } from "./previewDesktop";
import type { AgentArtifactOwner } from "../../domain/agentArtifact";
import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../../app/acpProjectionRegistry";
import type { AcpChatPage, ProjectGeneration } from "../../domain/acp";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

const target: PreviewTargetRef = {
  projectId: "project-preview",
  projectGeneration: 2,
  expectedTransitionEpoch: 4,
  rootId: "root-preview",
  scriptName: "dev",
};
const browseOwner = {
  projectId: target.projectId,
  chatId: "chat:preview",
  sessionId: "session-preview",
} as AgentArtifactOwner;

function seedBrowseOwner(owner: AgentArtifactOwner): void {
  const store = acpProjectionStoreFor(owner.chatId);
  const scope = {
    projectId: owner.projectId,
    projectGeneration: String(target.projectGeneration) as ProjectGeneration,
    chatId: owner.chatId,
  };
  store.getState().activateScope(scope);
  store.getState().applyAuthoritativePage(scope, {
    projectId: owner.projectId,
    chatId: owner.chatId,
    title: "Preview chat",
    projectionRevision: "1",
    currentSessionId: owner.sessionId,
    sessions: [],
    turns: [],
    nextCursor: null,
  } as unknown as AcpChatPage);
}

const {
  discoverPreviewTargets,
  getPreviewHealth,
  startPreviewTarget,
  stopPreviewTarget,
} = vi.hoisted(() => ({
  discoverPreviewTargets: vi.fn(
    async (
      _request: Pick<
        PreviewTargetRef,
        "projectId" | "projectGeneration" | "expectedTransitionEpoch"
      >,
    ) => ({
      projectId: target.projectId,
      transitionEpoch: target.expectedTransitionEpoch,
      targets: [target],
      truncated: false,
    }),
  ),
  getPreviewHealth: vi.fn(
    async (ref: PreviewTargetRef): Promise<PreviewHealthResponse> => ({
      ref,
      origin: "http://127.0.0.1:5173",
      healthy: true,
    }),
  ),
  startPreviewTarget: vi.fn(async (ref: PreviewTargetRef) => ({
    ref,
    origin: "http://127.0.0.1:5173",
  })),
  stopPreviewTarget: vi.fn(async (ref: PreviewTargetRef) => ({ ref })),
}));

vi.mock("./previewDesktop", () => ({
  discoverPreviewTargets,
  getPreviewHealth,
  startPreviewTarget,
  stopPreviewTarget,
}));

let previewRunning = false;

describe("PreviewSurface", () => {
  beforeEach(() => {
    previewRunning = false;
    discoverPreviewTargets.mockImplementation(async (request) => ({
      projectId: request.projectId,
      transitionEpoch: request.expectedTransitionEpoch,
      targets: [target],
      truncated: false,
    }));
    getPreviewHealth.mockImplementation(async (ref) => ({
      ref,
      origin: previewRunning ? "http://127.0.0.1:5173" : null,
      healthy: previewRunning,
    }));
    startPreviewTarget.mockImplementation(async (ref) => {
      previewRunning = true;
      return { ref, origin: "http://127.0.0.1:5173" };
    });
    stopPreviewTarget.mockImplementation(async (ref) => {
      previewRunning = false;
      return { ref };
    });
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    clearBrowseSessions();
    clearAcpProjectionRegistry();
    useWorkbenchStore.setState({
      project: null,
      projectGeneration: 0,
      nativeTransitionEpoch: 0,
      projectTransitioning: false,
    });
  });

  it("recovers an already-running Preview after the UI reloads", async () => {
    useWorkbenchStore.setState({
      project: { id: target.projectId, name: "Preview", roots: [] },
      projectGeneration: target.projectGeneration,
      nativeTransitionEpoch: target.expectedTransitionEpoch,
      projectTransitioning: false,
    });
    seedBrowseOwner(browseOwner);
    setHarnessBrowseOwner(browseOwner);
    previewRunning = true;

    render(<PreviewSurface />);

    await screen.findByRole("button", { name: "Stop" });
    expect(startPreviewTarget).not.toHaveBeenCalled();
    expect(browseSessionForOwner(browseOwner)?.href).toBe(
      "http://127.0.0.1:5173/",
    );
  });

  it("captures an owner established before deferred reload recovery begins", async () => {
    useWorkbenchStore.setState({
      project: { id: target.projectId, name: "Preview", roots: [] },
      projectGeneration: target.projectGeneration,
      nativeTransitionEpoch: target.expectedTransitionEpoch,
      projectTransitioning: false,
    });
    previewRunning = true;

    render(<PreviewSurface />);
    act(() => {
      seedBrowseOwner(browseOwner);
      setHarnessBrowseOwner(browseOwner);
    });

    await screen.findByRole("button", { name: "Stop" });
    expect(browseSessionForOwner(browseOwner)?.href).toBe(
      "http://127.0.0.1:5173/",
    );
  });

  it("captures the current owner when Refresh begins", async () => {
    useWorkbenchStore.setState({
      project: { id: target.projectId, name: "Preview", roots: [] },
      projectGeneration: target.projectGeneration,
      nativeTransitionEpoch: target.expectedTransitionEpoch,
      projectTransitioning: false,
    });
    seedBrowseOwner(browseOwner);
    setHarnessBrowseOwner(browseOwner);
    render(<PreviewSurface />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Start" })).toBeEnabled(),
    );

    const replacementOwner = {
      ...browseOwner,
      sessionId: "session-replacement",
    } as AgentArtifactOwner;
    act(() => {
      seedBrowseOwner(replacementOwner);
      setHarnessBrowseOwner(replacementOwner);
    });
    previewRunning = true;
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    await waitFor(() =>
      expect(browseSessionForOwner(replacementOwner)?.href).toBe(
        "http://127.0.0.1:5173/",
      ),
    );
    expect(browseSessionForOwner(browseOwner)).toBeNull();
  });

  it("ignores a healthy recovery response for a different target", async () => {
    useWorkbenchStore.setState({
      project: { id: target.projectId, name: "Preview", roots: [] },
      projectGeneration: target.projectGeneration,
      nativeTransitionEpoch: target.expectedTransitionEpoch,
      projectTransitioning: false,
    });
    seedBrowseOwner(browseOwner);
    setHarnessBrowseOwner(browseOwner);
    getPreviewHealth.mockResolvedValue({
      ref: { ...target, rootId: "root-other" },
      origin: "http://127.0.0.1:5173",
      healthy: true,
    });

    render(<PreviewSurface />);

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Start" })).toBeEnabled(),
    );
    expect(startPreviewTarget).not.toHaveBeenCalled();
    expect(browseSessionForOwner(browseOwner)).toBeNull();
  });

  it("stops a newly-started Preview that does not become healthy", async () => {
    useWorkbenchStore.setState({
      project: { id: target.projectId, name: "Preview", roots: [] },
      projectGeneration: target.projectGeneration,
      nativeTransitionEpoch: target.expectedTransitionEpoch,
      projectTransitioning: false,
    });
    getPreviewHealth.mockImplementation(async (ref) => ({
      ref,
      origin: null,
      healthy: false,
    }));
    render(<PreviewSurface />);

    fireEvent.click(await screen.findByRole("button", { name: "Start" }));

    await waitFor(() => expect(stopPreviewTarget).toHaveBeenCalledWith(target));
    expect(stopPreviewTarget).toHaveBeenCalledTimes(1);
    expect(await screen.findByRole("alert")).toHaveTextContent(/healthy/i);
    expect(screen.getByRole("button", { name: "Start" })).toBeEnabled();
  });

  it("does not reassign Preview recovery when the focused session changes", async () => {
    const pendingHealth = deferred<{
      ref: PreviewTargetRef;
      origin: string | null;
      healthy: boolean;
    }>();
    getPreviewHealth
      .mockReturnValueOnce(pendingHealth.promise)
      .mockResolvedValue({
        ref: target,
        origin: "http://127.0.0.1:5173",
        healthy: true,
      });
    useWorkbenchStore.setState({
      project: { id: target.projectId, name: "Preview", roots: [] },
      projectGeneration: target.projectGeneration,
      nativeTransitionEpoch: target.expectedTransitionEpoch,
      projectTransitioning: false,
    });
    seedBrowseOwner(browseOwner);
    setHarnessBrowseOwner(browseOwner);
    render(<PreviewSurface />);
    await waitFor(() => expect(getPreviewHealth).toHaveBeenCalledOnce());

    const replacementOwner = {
      ...browseOwner,
      sessionId: "session-replacement",
    } as AgentArtifactOwner;
    act(() => {
      seedBrowseOwner(replacementOwner);
      setHarnessBrowseOwner(replacementOwner);
    });
    await act(async () =>
      pendingHealth.resolve({
        ref: target,
        origin: "http://127.0.0.1:5173",
        healthy: true,
      }),
    );

    await screen.findByRole("button", { name: "Stop" });
    expect(getPreviewHealth).toHaveBeenCalledTimes(1);
    expect(browseSessionForOwner(browseOwner)).toBeNull();
    expect(browseSessionForOwner(replacementOwner)).toBeNull();
  });

  it("unmounts an inactive Preview surface and releases its active child", async () => {
    useWorkbenchStore.setState({
      project: { id: target.projectId, name: "Preview", roots: [] },
      projectGeneration: target.projectGeneration,
      nativeTransitionEpoch: target.expectedTransitionEpoch,
      projectTransitioning: false,
    });
    const Harness = ({ active }: { active: boolean }) =>
      active ? <PreviewSurface /> : null;
    const rendered = render(<Harness active />);

    await screen.findByRole("button", { name: "Start" });
    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    await waitFor(() =>
      expect(startPreviewTarget).toHaveBeenCalledWith(target),
    );
    rendered.rerender(<Harness active={false} />);

    expect(screen.queryByLabelText("Preview")).not.toBeInTheDocument();
    await waitFor(() => expect(stopPreviewTarget).toHaveBeenCalledWith(target));
  });

  it("stops a start that is still waiting when Preview unmounts", async () => {
    useWorkbenchStore.setState({
      project: { id: target.projectId, name: "Preview", roots: [] },
      projectGeneration: target.projectGeneration,
      nativeTransitionEpoch: target.expectedTransitionEpoch,
      projectTransitioning: false,
    });
    const starting = deferred<{ ref: PreviewTargetRef; origin: string }>();
    startPreviewTarget.mockReturnValueOnce(starting.promise);
    const Harness = ({ active }: { active: boolean }) =>
      active ? <PreviewSurface /> : null;
    const rendered = render(<Harness active />);

    await screen.findByRole("button", { name: "Start" });
    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    await waitFor(() =>
      expect(startPreviewTarget).toHaveBeenCalledWith(target),
    );
    rendered.rerender(<Harness active={false} />);

    await waitFor(() => expect(stopPreviewTarget).toHaveBeenCalledWith(target));
    await act(async () =>
      starting.resolve({ ref: target, origin: "http://127.0.0.1:5173" }),
    );
    expect(stopPreviewTarget).toHaveBeenCalledTimes(1);
    expect(getPreviewHealth).toHaveBeenCalledTimes(1);
  });

  it("resets an active target when the Project transition identity changes", async () => {
    const nextTarget: PreviewTargetRef = {
      ...target,
      projectId: "project-next",
      projectGeneration: 3,
      expectedTransitionEpoch: 5,
    };
    discoverPreviewTargets.mockImplementation(async (request) => ({
      projectId: request.projectId,
      transitionEpoch: request.expectedTransitionEpoch,
      targets: [
        request.expectedTransitionEpoch === target.expectedTransitionEpoch
          ? target
          : nextTarget,
      ],
      truncated: false,
    }));
    useWorkbenchStore.setState({
      project: { id: target.projectId, name: "Preview", roots: [] },
      projectGeneration: target.projectGeneration,
      nativeTransitionEpoch: target.expectedTransitionEpoch,
      projectTransitioning: false,
    });
    render(<PreviewSurface />);

    await screen.findByRole("button", { name: "Start" });
    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    await screen.findByRole("button", { name: "Stop" });

    act(() => {
      useWorkbenchStore.setState({
        project: { id: nextTarget.projectId, name: "Next", roots: [] },
        projectGeneration: nextTarget.projectGeneration,
        nativeTransitionEpoch: nextTarget.expectedTransitionEpoch,
      });
    });

    const nextStart = await screen.findByRole("button", { name: "Start" });
    fireEvent.click(nextStart);
    await waitFor(() =>
      expect(startPreviewTarget).toHaveBeenLastCalledWith(nextTarget),
    );
  });

  it("publishes a started loopback origin to the Sage browser for that chat", async () => {
    useWorkbenchStore.setState({
      project: { id: target.projectId, name: "Preview", roots: [] },
      projectGeneration: target.projectGeneration,
      nativeTransitionEpoch: target.expectedTransitionEpoch,
      projectTransitioning: false,
    });
    seedBrowseOwner(browseOwner);
    setHarnessBrowseOwner(browseOwner);
    render(<PreviewSurface />);
    await screen.findByRole("button", { name: "Start" });
    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    await waitFor(() =>
      expect(browseSessionForOwner(browseOwner)?.href).toBe(
        "http://127.0.0.1:5173/",
      ),
    );
  });

  it("does not publish a stale preview after the focused session changes", async () => {
    const started = deferred<{
      ref: PreviewTargetRef;
      origin: string;
    }>();
    startPreviewTarget.mockImplementationOnce(() => started.promise);
    useWorkbenchStore.setState({
      project: { id: target.projectId, name: "Preview", roots: [] },
      projectGeneration: target.projectGeneration,
      nativeTransitionEpoch: target.expectedTransitionEpoch,
      projectTransitioning: false,
    });
    seedBrowseOwner(browseOwner);
    setHarnessBrowseOwner(browseOwner);
    render(<PreviewSurface />);
    const start = await screen.findByRole("button", { name: "Start" });
    await waitFor(() => expect(getPreviewHealth).toHaveBeenCalledOnce());
    getPreviewHealth.mockResolvedValue({
      ref: target,
      origin: "http://127.0.0.1:5173",
      healthy: true,
    });
    await waitFor(() => expect(start).toBeEnabled());
    fireEvent.click(start);
    await waitFor(() => expect(startPreviewTarget).toHaveBeenCalledOnce());

    const replacementOwner = {
      ...browseOwner,
      sessionId: "session-replacement",
    } as AgentArtifactOwner;
    const projection = acpProjectionStoreFor(browseOwner.chatId);
    act(() => {
      projection.setState({
        page: {
          ...projection.getState().page!,
          currentSessionId: replacementOwner.sessionId,
        },
      });
      setHarnessBrowseOwner(replacementOwner);
    });
    started.resolve({
      ref: target,
      origin: "http://127.0.0.1:5173",
    });

    await waitFor(() => expect(getPreviewHealth).toHaveBeenCalledTimes(2));
    expect(browseSessionForOwner(browseOwner)).toBeNull();
    expect(browseSessionForOwner(replacementOwner)).toBeNull();
  });
});
