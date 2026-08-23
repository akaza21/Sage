import type {
  ProjectLayoutRequest,
  ResourceRef,
} from "../../../domain/workbench";
import { getWorkbenchPorts } from "../../../ports/workbenchRuntime";

export type AgentFileOperation = "created" | "updated" | "renamed" | "deleted";

export interface AgentFileIntent {
  operation: AgentFileOperation;
  resource: ResourceRef;
  source?: ResourceRef;
  basename?: string;
  content?: string;
  projectId: string;
  projectGeneration: number;
  expectedTransitionEpoch: number;
}

function requiredIntentText(value: string | undefined, label: string): string {
  if (!value) throw new Error(`Agent file ${label} is required`);
  return value;
}

function requiredIntentContent(value: string | undefined): string {
  if (value === undefined) throw new Error("Agent file content is required");
  return value;
}

function parentResource(resource: ResourceRef): ResourceRef {
  const separator = resource.relativePath.lastIndexOf("/");
  return {
    rootId: resource.rootId,
    relativePath:
      separator === -1 ? "" : resource.relativePath.slice(0, separator),
  };
}

async function currentProjectLayout(
  projectId: string,
): Promise<ProjectLayoutRequest> {
  const snapshot = await getWorkbenchPorts().project.getBootstrapSnapshot();
  if (snapshot.project?.id !== projectId) {
    throw new Error("Agent file intent Project is not active");
  }
  return {
    projectId,
    tabs: snapshot.tabs,
    activeTabId: snapshot.activeTabId,
    selectedRootId: snapshot.selectedRootId,
    selectedResource: snapshot.selectedResource,
  };
}

export async function applyAgentFileIntent(
  intent: AgentFileIntent,
): Promise<void> {
  const { project, editor } = getWorkbenchPorts();
  switch (intent.operation) {
    case "created": {
      const basename = requiredIntentText(intent.basename, "basename");
      const parent = parentResource(intent.resource);
      const target: ResourceRef = {
        rootId: parent.rootId,
        relativePath: `${parent.relativePath}${parent.relativePath ? "/" : ""}${basename}`,
      };
      if (
        target.rootId !== intent.resource.rootId ||
        target.relativePath !== intent.resource.relativePath
      ) {
        throw new Error(
          "Agent file create target does not match its typed resource",
        );
      }
      await project.createProjectFile({
        projectId: intent.projectId,
        projectGeneration: intent.projectGeneration,
        expectedTransitionEpoch: intent.expectedTransitionEpoch,
        parent,
        basename,
      });
      return;
    }
    case "updated": {
      const document = await editor.openEditorDocument(intent.resource);
      await editor.snapshotEditorBuffer({
        resource: intent.resource,
        base: document.base,
        lineEnding: document.lineEnding,
        content: requiredIntentContent(intent.content),
        revision: document.buffer.revision + 1,
      });
      await editor.saveEditorDocument(intent.resource);
      return;
    }
    case "renamed":
      await project.renameProjectFile({
        projectId: intent.projectId,
        projectGeneration: intent.projectGeneration,
        expectedTransitionEpoch: intent.expectedTransitionEpoch,
        source: requiredIntentResource(intent.source, "source"),
        basename: requiredIntentText(intent.basename, "basename"),
        currentLayout: await currentProjectLayout(intent.projectId),
      });
      return;
    case "deleted":
      await project.trashProjectResource({
        projectId: intent.projectId,
        projectGeneration: intent.projectGeneration,
        expectedTransitionEpoch: intent.expectedTransitionEpoch,
        source: intent.resource,
        kind: "file",
        currentLayout: await currentProjectLayout(intent.projectId),
      });
  }
}

function requiredIntentResource(
  value: ResourceRef | undefined,
  label: string,
): ResourceRef {
  if (!value) throw new Error(`Agent file ${label} is required`);
  return value;
}
