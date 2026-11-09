// Confirms and applies a previewed Workspace Replace: in the editor showing
// a File, in the unsaved buffer of a File open elsewhere, and natively on
// disk. This chunk loads with the first preview, so the Replace row carries
// none of it.

import { tabsWithEditorUpdate } from "../../app/editorSlice";
import { enqueueEditorOperation } from "../../app/editorOperations";
import { matchesProjectIntent } from "../../app/projectIntent";
import { useWorkbenchStore } from "../../app/store";
import { resourceKey, sameResource } from "../../domain/resourceIdentity";
import type { EditorDocument, ResourceRef } from "../../domain/workbench";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import {
  getFileEditorController,
  type EditorStateBridge,
} from "../files/fileEditorController";
import type {
  ReplaceFileProblem,
  ReplaceOutcome,
  ReplacePlan,
  WorkspaceReplaceGateway,
} from "./workspaceReplace";
import { problemFrom, unsavedEdits } from "./workspaceReplacePlan";

export { WorkspaceReplaceDialog } from "./WorkspaceReplaceDialog";

/** Puts `content` in the editor showing `file`, if it still shows `text`. */
async function replaceInEditor(
  file: ResourceRef,
  text: string,
  content: string,
) {
  const { replaceEditorText } = await import("../files/editorTextReplace");
  const result = replaceEditorText(resourceKey(file), text, content);
  if (result === "missing") throw new Error("its editor is not showing");
  if (result === "changed") {
    throw new Error("it was edited since the preview; search again");
  }
}

/** Writes `content` to the unsaved buffer of `file`, if it is as planned. */
async function replaceInBuffer(
  file: ResourceRef,
  planned: EditorDocument,
  content: string,
) {
  const editor = getWorkbenchPorts().editor;
  const document = await editor.openEditorDocument(file);
  if (
    document.buffer.revision !== planned.buffer.revision ||
    document.buffer.content !== planned.buffer.content
  ) {
    throw new Error("it was edited since the preview; search again");
  }
  const revision = document.buffer.revision + 1;
  const ack = await editor.snapshotEditorBuffer({
    resource: file,
    base: document.base,
    lineEnding: document.lineEnding,
    content,
    revision,
  });
  // Only an ack that says this text is stored proves the change.
  if (ack.applied !== true || ack.revision !== revision) {
    throw new Error("it was edited since the preview; search again");
  }
}

type AppliedOnDisk = Awaited<ReturnType<WorkspaceReplaceGateway["apply"]>>;

/** The editor showing `file`, while it has nothing unsaved. */
function cleanEditorOf(file: ResourceRef) {
  const editor = useWorkbenchStore.getState().activeEditor;
  if (
    editor?.phase !== "ready" ||
    !sameResource(editor.resource, file) ||
    editor.persisting ||
    editor.saving ||
    editor.revision !== editor.persistedRevision ||
    unsavedEdits(file)
  ) {
    return null;
  }
  return editor;
}

const editorBridge: EditorStateBridge = {
  getActiveEditor: () => useWorkbenchStore.getState().activeEditor,
  setActiveEditor: (editor, update) =>
    useWorkbenchStore.setState((state) => ({
      activeEditor: editor,
      tabs: tabsWithEditorUpdate(state.tabs, editor.resource, update),
    })),
};

/**
 * Opens `file` again in the editor after a disk Replace, so it shows what
 * is stored now: the File, or the replaced text kept as unsaved edits.
 * Only while the editor still shows `file`, clean, at `revision`, in the
 * project it had, checked once the queued operation runs, so an edit made
 * meanwhile is never dropped.
 */
function reopenIfUnchanged(file: ResourceRef, revision: number) {
  const { project, projectGeneration } = useWorkbenchStore.getState();
  const projectId = project?.id;
  if (!projectId) return Promise.resolve();
  const unchanged = () => {
    const state = useWorkbenchStore.getState();
    return (
      !state.projectTransitioning &&
      matchesProjectIntent(state, projectGeneration, projectId) &&
      cleanEditorOf(file)?.revision === revision
    );
  };
  return enqueueEditorOperation(async () => {
    if (!unchanged()) return;
    await getFileEditorController().load(file, editorBridge, unchanged);
  });
}

/** Applies `plan`, File by File; what could not be done is reported. */
export async function applyReplacePlan(
  plan: ReplacePlan,
  gateway: WorkspaceReplaceGateway,
): Promise<ReplaceOutcome> {
  const problems: ReplaceFileProblem[] = [];
  let matchCount = 0;
  let fileCount = 0;
  for (const file of plan.files) {
    const { target } = file;
    if (target.kind === "disk") continue;
    try {
      if (target.kind === "editor") {
        await replaceInEditor(file.resource, target.text, target.content);
      } else {
        await replaceInBuffer(file.resource, target.document, target.content);
      }
      matchCount += file.count;
      fileCount += 1;
    } catch (error) {
      problems.push(problemFrom(file.resource, "stale", error));
    }
  }
  let onDisk = plan.files.filter((file) => file.target.kind === "disk");
  if (!onDisk.length) return { matchCount, fileCount, problems };
  // What was typed since the preview is stored first, so a File the user
  // has edited meanwhile is seen as such and left to them.
  await useWorkbenchStore
    .getState()
    .flushActiveEditor()
    .catch(() => undefined);
  const reopen = new Map<string, number>();
  let applied: AppliedOnDisk;
  try {
    applied = await enqueueEditorOperation(() => {
      onDisk = onDisk.filter((file) => {
        if (!unsavedEdits(file.resource)) return true;
        problems.push({
          resource: file.resource,
          kind: "stale",
          message: "it was edited since the preview; search again",
        });
        return false;
      });
      for (const file of onDisk) {
        const editor = cleanEditorOf(file.resource);
        if (editor) reopen.set(resourceKey(file.resource), editor.revision);
      }
      if (!onDisk.length) {
        return Promise.resolve<AppliedOnDisk>({ replaced: [], problems: [] });
      }
      return gateway.apply({
        projectId: plan.projectId,
        expectedTransitionEpoch: plan.transitionEpoch,
        spec: plan.spec,
        files: onDisk.map((file) => ({
          resource: file.resource,
          fingerprint:
            file.target.kind === "disk" ? file.target.fingerprint : "",
          ...(plan.only ? { only: plan.only } : {}),
        })),
      });
    });
  } catch (error) {
    // The editors' changes are made and stay counted; the Files on disk
    // are reported, since none of them is known to be written.
    for (const file of onDisk) {
      problems.push(problemFrom(file.resource, "refused", error));
    }
    return { matchCount, fileCount, problems };
  }
  problems.push(...applied.problems);
  for (const file of applied.replaced) {
    matchCount += file.count;
    fileCount += 1;
  }
  // A clean editor on a File replaced on disk shows it again, and one
  // whose replaced text was kept as unsaved edits shows those.
  const changed = [
    ...applied.replaced.map((file) => file.resource),
    ...applied.problems
      .filter((problem) => problem.kind === "keptUnsaved")
      .map((problem) => problem.resource),
  ];
  for (const file of changed) {
    const revision = reopen.get(resourceKey(file));
    if (revision === undefined) continue;
    try {
      await reopenIfUnchanged(file, revision);
    } catch {
      // The File is replaced; a failed reopen leaves the editor as it is.
    }
  }
  return { matchCount, fileCount, problems };
}
