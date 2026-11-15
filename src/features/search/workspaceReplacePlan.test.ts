import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import type {
  EditorDocument,
  ResourceRef,
  WorkbenchTab,
} from "../../domain/workbench";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import type { ReplaceSpec, WorkspaceReplaceGateway } from "./workspaceReplace";
import { applyReplacePlan } from "./workspaceReplaceApply";
import { planReplace, sameLine } from "./workspaceReplacePlan";

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
        applied: true,
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
    vi.spyOn(editor, "snapshotEditorBuffer").mockResolvedValue({
      revision: 4,
      applied: false,
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
        applied: true,
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

  it("shows a clean editor the replaced File, read again", async () => {
    const reread = vi
      .spyOn(getWorkbenchPorts().editor, "openEditorDocument")
      .mockResolvedValue({ ...document(alpha, "new\n", 0), dirty: false });
    const replace = gateway();

    await applyReplacePlan(await plan(replace, [alpha]), replace);

    expect(reread).toHaveBeenCalledWith(alpha);
    expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
      phase: "ready",
      content: "new\n",
    });
  });

  it("leaves an editor typed in while the Replace ran", async () => {
    const reread = vi.spyOn(getWorkbenchPorts().editor, "openEditorDocument");
    const replace = gateway();
    const planned = await plan(replace, [alpha]);
    vi.mocked(replace.apply).mockImplementation(async (request) => {
      useWorkbenchStore.setState({
        activeEditor: { ...readyEditor(alpha, 3), content: "typed\n" },
      } as unknown as Partial<StoreState>);
      return gateway().apply(request);
    });

    const outcome = await applyReplacePlan(planned, replace);

    expect(outcome.fileCount).toBe(1);
    expect(reread).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
      content: "typed\n",
      revision: 3,
    });
  });

  it("shows the replaced text kept as unsaved edits when the write was not proven", async () => {
    vi.spyOn(
      getWorkbenchPorts().editor,
      "openEditorDocument",
    ).mockResolvedValue(document(alpha, "new\n", 1));
    const replace = gateway();
    const planned = await plan(replace, [alpha]);
    vi.mocked(replace.apply).mockResolvedValue({
      replaced: [],
      problems: [
        { resource: alpha, kind: "keptUnsaved", message: "kept as unsaved" },
      ],
    });

    const outcome = await applyReplacePlan(planned, replace);

    expect(outcome.problems).toEqual([
      expect.objectContaining({ resource: alpha, kind: "keptUnsaved" }),
    ]);
    const state = useWorkbenchStore.getState();
    expect(state.activeEditor).toMatchObject({ content: "new\n" });
    expect(state.tabs.find((tab) => tab.id === "file:alpha.ts")?.dirty).toBe(
      true,
    );
  });

  it("stores what was typed since the preview first and leaves that File", async () => {
    const replace = gateway();
    const planned = await plan(replace, [alpha]);
    const order: string[] = [];
    useWorkbenchStore.setState({
      flushActiveEditor: vi.fn().mockImplementation(async () => {
        order.push("flush");
        useWorkbenchStore.setState({
          tabs: [fileTab(alpha, true), fileTab(beta, true)],
        });
      }),
    });

    const outcome = await applyReplacePlan(planned, replace);

    expect(order).toEqual(["flush"]);
    expect(replace.apply).not.toHaveBeenCalled();
    expect(outcome.fileCount).toBe(0);
    expect(outcome.problems).toEqual([
      expect.objectContaining({ resource: alpha, kind: "stale" }),
    ]);
  });
});

describe("replacing one match on disk", () => {
  it("is refused when the previewed line is not the line Search showed", async () => {
    const replace = gateway();
    vi.mocked(replace.preview).mockResolvedValue({
      files: [
        {
          resource: alpha,
          fingerprint: "disk",
          count: 1,
          lines: [
            { lineNumber: 2, columnNumber: 1, before: "old x", after: "new x" },
          ],
          linesTruncated: false,
        },
      ],
      problems: [],
      truncated: false,
    });

    const changed = await plan(replace, [alpha], {
      lineNumber: 2,
      columnNumber: 1,
      line: "old line",
    });
    expect(changed.files).toEqual([]);
    expect(changed.problems).toEqual([
      expect.objectContaining({ resource: alpha, kind: "stale" }),
    ]);

    const same = await plan(replace, [alpha], {
      lineNumber: 2,
      columnNumber: 1,
      line: "old x",
    });
    expect(same.files).toHaveLength(1);
  });
});

describe("sameLine", () => {
  it("trims as Search's Rust preview does", () => {
    // Rust trims U+0085 but not U+FEFF; String.prototype.trim does the
    // reverse.
    expect(sameLine("\u0085old line\u0085", "old line")).toBe(true);
    expect(sameLine("﻿old line", "old line")).toBe(false);
    expect(sameLine("﻿old line", "﻿old line")).toBe(true);
    expect(sameLine("  old line that goes on", "old line…")).toBe(true);
    expect(sameLine("old other", "old line…")).toBe(false);
  });
});
