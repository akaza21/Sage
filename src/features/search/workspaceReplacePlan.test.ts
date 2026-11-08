import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import type {
  EditorDocument,
  ResourceRef,
  WorkbenchTab,
} from "../../domain/workbench";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import type { ReplaceSpec, WorkspaceReplaceGateway } from "./workspaceReplace";
import { applyReplacePlan, planReplace } from "./workspaceReplacePlan";

type StoreState = ReturnType<typeof useWorkbenchStore.getState>;

const initialStore = useWorkbenchStore.getState();
const alpha: ResourceRef = { rootId: "root-a", relativePath: "alpha.ts" };
const beta: ResourceRef = { rootId: "root-a", relativePath: "beta.ts" };
const spec: ReplaceSpec = {
  query: "old",
  replacement: "new",
  caseSensitive: false,
  wholeWord: false,
  useRegex: false,
  preserveCase: false,
};

const fileTab = (resource: ResourceRef, dirty: boolean): WorkbenchTab => ({
  id: `file:${resource.relativePath}`,
  kind: "file",
  title: resource.relativePath,
  position: 1,
  pinned: false,
  dirty,
  resource,
});

function document(
  resource: ResourceRef,
  content: string,
  revision: number,
): EditorDocument {
  const version = { fingerprint: "disk", byteLen: 4, modifiedAt: null };
  return {
    resource,
    disk: version,
    base: version,
    buffer: {
      fingerprint: `buffer-${revision}`,
      byteLen: content.length,
      revision,
      content,
    },
    lineEnding: "lf",
    dirty: true,
    conflict: null,
  };
}

function readyEditor(resource: ResourceRef, revision: number) {
  return {
    phase: "ready" as const,
    resource,
    document: document(resource, "old\n", 0),
    content: "old\n",
    revision,
    persistedRevision: revision,
    persisting: false,
    saving: false,
    message: null,
  };
}

function gateway(): WorkspaceReplaceGateway {
  return {
    preview: vi.fn().mockImplementation(({ resources }) =>
      Promise.resolve({
        files: (resources as ResourceRef[]).map((resource) => ({
          resource,
          fingerprint: "disk",
          count: 1,
          lines: [],
          linesTruncated: false,
        })),
        problems: [],
        truncated: false,
      }),
    ),
    apply: vi.fn().mockImplementation(({ files }) =>
      Promise.resolve({
        replaced: (files as { resource: ResourceRef }[]).map((file) => ({
          resource: file.resource,
          count: 1,
          fingerprint: "replaced",
        })),
        problems: [],
      }),
    ),
    replaceText: vi.fn().mockImplementation(({ text }) =>
      Promise.resolve({
        content: (text as string).replaceAll("old", "new"),
        count: 1,
        lines: [],
        linesTruncated: false,
      }),
    ),
  };
}

function plan(
  replace: WorkspaceReplaceGateway,
  resources: ResourceRef[],
  only?: { lineNumber: number; columnNumber: number; line: string },
) {
  return planReplace({
    gateway: replace,
    projectId: "project-a",
    transitionEpoch: 1,
    spec,
    resources,
    ...(only
      ? {
          only: {
            lineNumber: only.lineNumber,
            columnNumber: only.columnNumber,
          },
          onlyLine: only.line,
        }
      : {}),
  });
}

beforeEach(() => {
  useWorkbenchStore.setState({
    project: { id: "project-a", roots: [{ id: "root-a" }] },
    projectTransitioning: false,
    tabs: [fileTab(alpha, false), fileTab(beta, true)],
    activeTabId: fileTab(alpha, false).id,
    activeEditor: readyEditor(alpha, 2),
  } as unknown as Partial<StoreState>);
});

afterEach(() => {
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initialStore, true);
});

describe("a File with unsaved edits in another tab", () => {
  it("is replaced in its buffer as the next revision", async () => {
    const editor = getWorkbenchPorts().editor;
    vi.spyOn(editor, "openEditorDocument").mockResolvedValue(
      document(beta, "old line\n", 3),
    );
    const snapshot = vi
      .spyOn(editor, "snapshotEditorBuffer")
      .mockImplementation(async (request) => ({
        revision: request.revision,
        persistedAt: 1,
        dirty: true,
      }));
    const replace = gateway();

    const planned = await plan(replace, [beta]);
    expect(planned.files[0].target.kind).toBe("buffer");
    const outcome = await applyReplacePlan(planned, replace);

    expect(outcome).toEqual({ matchCount: 1, fileCount: 1, problems: [] });
    expect(snapshot).toHaveBeenCalledWith(
      expect.objectContaining({ content: "new line\n", revision: 4 }),
    );
    expect(replace.apply).not.toHaveBeenCalled();
  });

  it("is reported stale when the buffer keeps another write", async () => {
    const editor = getWorkbenchPorts().editor;
    vi.spyOn(editor, "openEditorDocument").mockResolvedValue(
      document(beta, "old line\n", 3),
    );
    // The buffer acknowledges the revision it already had: another write
    // reached it first.
    vi.spyOn(editor, "snapshotEditorBuffer").mockResolvedValue({
      revision: 3,
      persistedAt: 1,
      dirty: true,
    });
    const replace = gateway();

    const outcome = await applyReplacePlan(
      await plan(replace, [beta]),
      replace,
    );

    expect(outcome.fileCount).toBe(0);
    expect(outcome.matchCount).toBe(0);
    expect(outcome.problems).toEqual([
      expect.objectContaining({ resource: beta, kind: "stale" }),
    ]);
  });

  it("is refused for one match when its line is not the line Search showed", async () => {
    vi.spyOn(
      getWorkbenchPorts().editor,
      "openEditorDocument",
    ).mockResolvedValue(document(beta, "first\nold changed\n", 3));
    const replace = gateway();

    const planned = await plan(replace, [beta], {
      lineNumber: 2,
      columnNumber: 1,
      line: "old line",
    });

    expect(planned.files).toEqual([]);
    expect(planned.problems).toEqual([
      expect.objectContaining({ resource: beta, kind: "refused" }),
    ]);
    expect(replace.replaceText).not.toHaveBeenCalled();

    const same = await plan(replace, [beta], {
      lineNumber: 2,
      columnNumber: 1,
      line: "old ch…",
    });
    expect(same.files).toHaveLength(1);
  });
});

describe("applying a Replace", () => {
  it("keeps what the editors did when the native apply fails", async () => {
    const editor = getWorkbenchPorts().editor;
    vi.spyOn(editor, "openEditorDocument").mockResolvedValue(
      document(beta, "old\n", 3),
    );
    vi.spyOn(editor, "snapshotEditorBuffer").mockImplementation(
      async (request) => ({
        revision: request.revision,
        persistedAt: 1,
        dirty: true,
      }),
    );
    const replace = gateway();
    const other: ResourceRef = { rootId: "root-a", relativePath: "other.ts" };
    const planned = await plan(replace, [beta, other]);
    vi.mocked(replace.apply).mockRejectedValue(new Error("the app closed"));

    const outcome = await applyReplacePlan(planned, replace);

    expect(outcome.matchCount).toBe(1);
    expect(outcome.fileCount).toBe(1);
    expect(outcome.problems).toEqual([
      { resource: other, kind: "refused", message: "the app closed" },
    ]);
  });

  it("reloads a clean editor only at the revision it had", async () => {
    const discard = vi.fn().mockResolvedValue(true);
    useWorkbenchStore.setState({ discardActiveEditor: discard });
    const replace = gateway();

    await applyReplacePlan(await plan(replace, [alpha]), replace);

    expect(discard).toHaveBeenCalledWith({ resource: alpha, revision: 2 });
  });
});

describe("a guarded discard", () => {
  it("leaves an editor that changed while it waited", async () => {
    const reread = vi
      .spyOn(getWorkbenchPorts().editor, "discardEditorBuffer")
      .mockResolvedValue({ ...document(alpha, "new\n", 0), dirty: false });
    const store = useWorkbenchStore.getState();

    // Typed since the reload was asked for.
    await expect(
      store.discardActiveEditor({ resource: alpha, revision: 1 }),
    ).resolves.toBe(false);
    useWorkbenchStore.setState({
      tabs: [fileTab(alpha, true), fileTab(beta, true)],
    });
    await expect(
      store.discardActiveEditor({ resource: alpha, revision: 2 }),
    ).resolves.toBe(false);
    expect(reread).not.toHaveBeenCalled();

    useWorkbenchStore.setState({
      tabs: [fileTab(alpha, false), fileTab(beta, true)],
    });
    await expect(
      store.discardActiveEditor({ resource: alpha, revision: 2 }),
    ).resolves.toBe(true);
    expect(reread).toHaveBeenCalledWith(alpha);
  });
});
