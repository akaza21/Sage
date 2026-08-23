import { readFile } from "node:fs/promises";
import path from "node:path";

import { beforeEach, describe, expect, it, vi } from "vitest";

import type {
  BootstrapSnapshot,
  EditorDocument,
} from "../../../domain/workbench";
import { getWorkbenchPorts } from "../../../ports/workbenchRuntime";

const resource = { rootId: "root-a", relativePath: "notes/agent.txt" };
const source = { rootId: "root-a", relativePath: "notes/before.txt" };
const version = { fingerprint: "base", byteLen: 5, modifiedAt: null };
const document: EditorDocument = {
  resource,
  disk: version,
  base: version,
  buffer: { fingerprint: "base", byteLen: 5, revision: 7, content: "before" },
  lineEnding: "lf",
  dirty: false,
  conflict: null,
};
const snapshot: BootstrapSnapshot = {
  transitionEpoch: 11,
  projects: [],
  project: { id: "project-a", name: "Project A", roots: [] },
  tabs: [],
  archivedChats: [],
  activeTabId: null,
  selectedRootId: "root-a",
  selectedResource: resource,
  harnesses: [],
  sessionId: null,
  selectedHarnessId: null,
  selectedModelId: null,
  messages: [],
  repositories: { roots: [], truncated: false, elapsedMs: 0 },
  turnChanges: [],
  recoveredBuffers: [],
  recoveryNotice: null,
};

import { applyAgentFileIntent } from "./agentFileOps";

const { project, editor } = getWorkbenchPorts();

beforeEach(() => {
  vi.spyOn(project, "createProjectFile").mockResolvedValue({} as never);
  vi.spyOn(project, "getBootstrapSnapshot").mockResolvedValue(snapshot);
  vi.spyOn(project, "renameProjectFile").mockResolvedValue({} as never);
  vi.spyOn(project, "trashProjectResource").mockResolvedValue({} as never);
  vi.spyOn(editor, "openEditorDocument").mockResolvedValue(document);
  vi.spyOn(editor, "snapshotEditorBuffer").mockResolvedValue({} as never);
  vi.spyOn(editor, "saveEditorDocument").mockResolvedValue({} as never);
});

describe("AGENT-1 typed file-operation seam", () => {
  it("apply_created_file_uses_workspace_create_not_raw_fs", async () => {
    await applyAgentFileIntent({
      operation: "created",
      resource,
      basename: "agent.txt",
      projectId: "project-a",
      projectGeneration: 3,
      expectedTransitionEpoch: 11,
    });

    expect(project.createProjectFile).toHaveBeenCalledWith({
      projectId: "project-a",
      projectGeneration: 3,
      expectedTransitionEpoch: 11,
      parent: { rootId: "root-a", relativePath: "notes" },
      basename: "agent.txt",
    });
  });

  it("apply_created_file_at_root_uses_an_empty_parent_path", async () => {
    await applyAgentFileIntent({
      operation: "created",
      resource: { rootId: "root-a", relativePath: "agent.txt" },
      basename: "agent.txt",
      projectId: "project-a",
      projectGeneration: 3,
      expectedTransitionEpoch: 11,
    });

    expect(project.createProjectFile).toHaveBeenCalledWith({
      projectId: "project-a",
      projectGeneration: 3,
      expectedTransitionEpoch: 11,
      parent: { rootId: "root-a", relativePath: "" },
      basename: "agent.txt",
    });
  });

  it("apply_created_file_accepts_a_matching_recomposed_target", async () => {
    await applyAgentFileIntent({
      operation: "created",
      resource,
      basename: "agent.txt",
      projectId: "project-a",
      projectGeneration: 3,
      expectedTransitionEpoch: 11,
    });

    expect(project.createProjectFile).toHaveBeenCalledOnce();
  });

  it("apply_created_file_rejects_a_mismatched_basename", async () => {
    await expect(
      applyAgentFileIntent({
        operation: "created",
        resource,
        basename: "other.txt",
        projectId: "project-a",
        projectGeneration: 3,
        expectedTransitionEpoch: 11,
      }),
    ).rejects.toThrow("does not match its typed resource");

    expect(project.createProjectFile).not.toHaveBeenCalled();
  });

  it("apply_updated_file_uses_editor_snapshot_and_save", async () => {
    await applyAgentFileIntent({
      operation: "updated",
      resource,
      content: "after",
      projectId: "project-a",
      projectGeneration: 3,
      expectedTransitionEpoch: 11,
    });

    expect(editor.openEditorDocument).toHaveBeenCalledWith(resource);
    expect(editor.snapshotEditorBuffer).toHaveBeenCalledWith({
      resource,
      base: version,
      lineEnding: "lf",
      content: "after",
      revision: 8,
    });
    expect(editor.saveEditorDocument).toHaveBeenCalledWith(resource);
  });

  it("apply_updated_file_allows_empty_content_to_truncate", async () => {
    await applyAgentFileIntent({
      operation: "updated",
      resource,
      content: "",
      projectId: "project-a",
      projectGeneration: 3,
      expectedTransitionEpoch: 11,
    });

    expect(editor.snapshotEditorBuffer).toHaveBeenCalledWith({
      resource,
      base: version,
      lineEnding: "lf",
      content: "",
      revision: 8,
    });
    expect(editor.saveEditorDocument).toHaveBeenCalledWith(resource);
  });

  it("apply_renamed_file_uses_workspace_rename", async () => {
    await applyAgentFileIntent({
      operation: "renamed",
      resource,
      source,
      basename: "agent.txt",
      projectId: "project-a",
      projectGeneration: 3,
      expectedTransitionEpoch: 11,
    });

    expect(project.renameProjectFile).toHaveBeenCalledWith({
      projectId: "project-a",
      projectGeneration: 3,
      expectedTransitionEpoch: 11,
      source,
      basename: "agent.txt",
      currentLayout: {
        projectId: "project-a",
        tabs: [],
        activeTabId: null,
        selectedRootId: "root-a",
        selectedResource: resource,
      },
    });
  });

  it("apply_deleted_file_uses_workspace_trash", async () => {
    await applyAgentFileIntent({
      operation: "deleted",
      resource,
      projectId: "project-a",
      projectGeneration: 3,
      expectedTransitionEpoch: 11,
    });

    expect(project.trashProjectResource).toHaveBeenCalledWith({
      projectId: "project-a",
      projectGeneration: 3,
      expectedTransitionEpoch: 11,
      source: resource,
      kind: "file",
      currentLayout: {
        projectId: "project-a",
        tabs: [],
        activeTabId: null,
        selectedRootId: "root-a",
        selectedResource: resource,
      },
    });
  });

  it("typed_file_ops_do_not_call_later_permission_as_the_apply_path", async () => {
    const sourceText = await readFile(
      path.join(import.meta.dirname, "agentFileOps.ts"),
      "utf8",
    );

    expect(sourceText).not.toContain("resolve_later_permission");
    expect(sourceText).not.toMatch(/\b(?:codex|claude|pi|oh-my-pi)\b/i);
    expect(sourceText).not.toMatch(/services\/desktop|from ["'].*desktop/);
  });
});
