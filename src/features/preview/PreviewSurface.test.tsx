import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import { browseSessionForChat, clearBrowseSessions } from "./harnessBrowse";
import { PreviewSurface } from "./PreviewSurface";
import type { PreviewTargetRef } from "./previewDesktop";

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
  getPreviewHealth: vi.fn(async (ref: PreviewTargetRef) => ({
    ref,
    origin: "http://127.0.0.1:5173",
    healthy: true,
  })),
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

describe("PreviewSurface", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    clearBrowseSessions();
    useWorkbenchStore.setState({
      project: null,
      projectGeneration: 0,
      nativeTransitionEpoch: 0,
      projectTransitioning: false,
    });
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
    expect(getPreviewHealth).not.toHaveBeenCalled();
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
    startPreviewTarget.mockImplementation(async (ref) => ({
      ref,
      origin: "http://127.0.0.1:5173",
    }));
    getPreviewHealth.mockImplementation(async (ref) => ({
      ref,
      origin: "http://127.0.0.1:5173",
      healthy: true,
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
    render(<PreviewSurface browseChatId="chat:preview" />);
    await screen.findByRole("button", { name: "Start" });
    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    await waitFor(() =>
      expect(browseSessionForChat("chat:preview")?.href).toBe(
        "http://127.0.0.1:5173/",
      ),
    );
  });
});
