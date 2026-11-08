import { lazy, Suspense } from "react";

import { LoaderCircle } from "../../ui/leanIcons";
import type { WorkspaceReplace } from "./useWorkspaceReplace";

// The preview dialog loads with the first preview, not with the Search panel.
const WorkspaceReplaceDialog = lazy(() =>
  import("./WorkspaceReplaceDialog").then((module) => ({
    default: module.WorkspaceReplaceDialog,
  })),
);

/** The preview awaiting a confirm, while there is one. */
export function WorkspaceReplaceConfirm({
  replace,
  rootName,
}: {
  replace: WorkspaceReplace;
  rootName: (rootId: string) => string;
}) {
  const { step } = replace;
  if (step.kind !== "confirming" && step.kind !== "applying") return null;
  return (
    <Suspense fallback={null}>
      <WorkspaceReplaceDialog
        plan={step.plan}
        applying={step.kind === "applying"}
        rootName={rootName}
        onConfirm={() => void replace.confirm(step.plan)}
        onCancel={replace.cancel}
      />
    </Suspense>
  );
}

/** A preview being prepared, or what the last Replace did. */
export function WorkspaceReplaceStatus({
  replace,
}: {
  replace: WorkspaceReplace;
}) {
  const { step, report } = replace;
  return (
    <>
      {step.kind === "planning" ? (
        <p className="workspace-search__empty workspace-search__progress">
          <LoaderCircle size={14} aria-hidden="true" />
          Preparing the Replace preview...
        </p>
      ) : null}
      {report ? (
        <div className="workspace-replace__report" role="status">
          <p>{report.message}</p>
          {report.problems.length ? (
            <ul aria-label="Files left as they are">
              {report.problems.map((problem) => (
                <li
                  key={`${problem.resource.rootId}\0${problem.resource.relativePath}`}
                >
                  {problem.resource.relativePath}: {problem.message}
                </li>
              ))}
            </ul>
          ) : null}
        </div>
      ) : null}
    </>
  );
}
