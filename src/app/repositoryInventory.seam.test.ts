import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { diffCacheKey, isNestedRepository } from "../domain/diffIdentity";
import { resourceKey } from "../domain/resourceIdentity";
import type { FileDiff, RepositoryRef, ResourceRef } from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { useWorkbenchStore } from "./store";

const file: ResourceRef = { rootId: "root-a", relativePath: "src/lib.rs" };
const nested: RepositoryRef = {
  rootId: "root-a",
  relativePath: "vendor/nested",
};
const emptyDiff: FileDiff = {
  resource: file,
  hunks: [],
  binary: false,
  truncated: false,
};

describe("IDE-3 nested Diff identity seam", () => {
  beforeEach(() => {
    useWorkbenchStore.setState({
      phase: "ready",
      nativeTransitionEpoch: 0,
      project: {
        id: "project-a",
        name: "A",
        roots: [
          {
            id: "root-a",
            name: "A",
            ordinal: 0,
            availability: "available",
            message: null,
          },
        ],
      },
      tabs: [
        {
          id: "chat:a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: true,
          dirty: false,
        },
      ],
      activeTabId: "chat:a",
      selectedRootId: "root-a",
      selectedResource: null,
      projectGeneration: 0,
      projectTransitioning: false,
      fileCreateInFlight: false,
      folderCreateInFlight: false,
      fileRenameInFlight: false,
      resourceTrashInFlight: false,
      diffs: {},
      requestGenerations: {},
      requestIntentSequence: 0,
      error: null,
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("does not fall through V1 getFileDiff for a nested Diff", async () => {
    expect(isNestedRepository(nested)).toBe(true);
    const repository = getWorkbenchPorts().repository;
    const nestedDiff = vi
      .spyOn(repository, "getNestedRepositoryFileDiff")
      .mockResolvedValue({
        projectId: "project-a",
        transitionEpoch: 0,
        diff: emptyDiff,
      });
    const v1 = vi.spyOn(repository, "getFileDiff").mockResolvedValue(emptyDiff);

    await useWorkbenchStore.getState().openDiff(file, nested);
    await vi.waitFor(() => {
      expect(nestedDiff).toHaveBeenCalled();
    });

    const tab = useWorkbenchStore
      .getState()
      .tabs.find((candidate) => candidate.kind === "diff");
    expect(tab?.repository).toEqual(nested);
    expect(diffCacheKey(tab?.repository, file)).not.toBe(resourceKey(file));
    expect(v1).not.toHaveBeenCalled();
    expect(nestedDiff).toHaveBeenCalledWith(
      expect.objectContaining({ ref: nested, resource: file }),
    );

    await useWorkbenchStore.getState().openDiff(file);
    await vi.waitFor(() => {
      expect(v1).toHaveBeenCalledWith(file);
    });
  });

  it("restored tabs retain repository identity through cache keys", () => {
    const restored = {
      id: "diff:nested",
      kind: "diff" as const,
      title: "lib.rs",
      position: 1,
      pinned: false,
      dirty: false,
      resource: file,
      repository: nested,
    };
    useWorkbenchStore.setState({
      tabs: [restored],
      activeTabId: restored.id,
    });
    const tab = useWorkbenchStore.getState().tabs[0];
    expect(tab.repository).toEqual(nested);
    expect(diffCacheKey(tab.repository, file)).toBe(
      `${resourceKey(nested)}\0${resourceKey(file)}`,
    );
  });
});
