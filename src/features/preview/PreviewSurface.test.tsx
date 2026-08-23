import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
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

const discoverPreviewTargets = vi.fn(async () => ({
  projectId: target.projectId,
  transitionEpoch: target.expectedTransitionEpoch,
  targets: [target],
  truncated: false,
}));
const getPreviewHealth = vi.fn(async () => ({
  ref: target,
  origin: "http://127.0.0.1:5173",
  healthy: true,
}));
const startPreviewTarget = vi.fn(async () => ({
  ref: target,
  origin: "http://127.0.0.1:5173",
}));
const stopPreviewTarget = vi.fn(async () => ({ ref: target }));

vi.mock("./previewDesktop", () => ({
  discoverPreviewTargets,
  getPreviewHealth,
  startPreviewTarget,
  stopPreviewTarget,
}));

describe("PreviewSurface", () => {
  afterEach(() => {
    vi.clearAllMocks();
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
});
