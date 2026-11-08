// Workspace Replace in the Search panel: the Replace row, the per-File and
// per-match Replace buttons, the preview awaiting a confirm and what the last
// Replace did. The panel loads this chunk only when the user opens Replace
// (or preloads it when they reach for the toggle), so Search alone carries
// none of it. The panel keeps the Replace state, so it outlives a close.

import type { Dispatch, SetStateAction } from "react";

import type { ResourceRef } from "../../domain/workbench";
import { LoaderCircle, Replace, ReplaceAll } from "../../ui/leanIcons";
import type { WorkspaceSearchState } from "./useWorkspaceSearch";
import type {
  MatchPosition,
  ReplaceFileProblem,
  ReplacePlan,
  WorkspaceReplaceGateway,
} from "./workspaceReplace";
import { WorkspaceReplaceDialog } from "./WorkspaceReplaceDialog";
import {
  applyReplacePlan,
  planReplace,
  workspaceReplaceDesktop,
} from "./workspaceReplacePlan";
import type { WorkspaceSearchMatch } from "./workspaceSearchDomain";
import "./workspace-replace.css";

export type ReplaceStep =
  | { kind: "idle" }
  | { kind: "planning" }
  | { kind: "confirming" | "applying"; plan: ReplacePlan };

export interface ReplaceReport {
  message: string;
  problems: ReplaceFileProblem[];
}

export interface ReplaceState {
  replacement: string;
  preserveCase: boolean;
  step: ReplaceStep;
  report: ReplaceReport | null;
}

const initialReplaceState: ReplaceState = {
  replacement: "",
  preserveCase: false,
  step: { kind: "idle" },
  report: null,
};

/** What every part needs: the panel's Replace state and its Search. */
export interface ReplaceContext {
  state: ReplaceState | undefined;
  setState: Dispatch<SetStateAction<ReplaceState | undefined>>;
  search: WorkspaceSearchState;
  /** The native gateway when left out. */
  gateway?: WorkspaceReplaceGateway;
  onReplaced: () => void;
  /** The first match of each File Search shows, in order. */
  files: ResourceRef[];
  projectId: string | null;
}

function current(context: ReplaceContext) {
  return context.state ?? initialReplaceState;
}

function update(context: ReplaceContext, change: Partial<ReplaceState>) {
  context.setState((state) => ({
    ...(state ?? initialReplaceState),
    ...change,
  }));
}

function errorText(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  return message.replace(/^invalid request: /, "");
}

function counted(count: number, word: string, plural: string) {
  return `${count} ${count === 1 ? word : plural}`;
}

/** Previews a Replace of `resources`, or of the one match at `only`. */
async function start(
  context: ReplaceContext,
  resources: ResourceRef[],
  only?: MatchPosition,
) {
  const { search } = context;
  const { replacement, preserveCase, step } = current(context);
  const response = search.response;
  if (!response || !resources.length || step.kind !== "idle") return;
  update(context, { report: null, step: { kind: "planning" } });
  try {
    const plan = await planReplace({
      gateway: context.gateway ?? workspaceReplaceDesktop,
      projectId: response.projectId,
      transitionEpoch: response.transitionEpoch,
      spec: {
        query: response.query,
        replacement,
        caseSensitive: search.caseSensitive,
        wholeWord: search.wholeWord,
        useRegex: search.useRegex,
        preserveCase,
      },
      resources,
      only,
    });
    update(context, { step: { kind: "confirming", plan } });
  } catch (error) {
    update(context, {
      step: { kind: "idle" },
      report: {
        message: `Couldn't preview the Replace: ${errorText(error)}`,
        problems: [],
      },
    });
  }
}

async function confirm(context: ReplaceContext, plan: ReplacePlan) {
  update(context, { step: { kind: "applying", plan } });
  let report: ReplaceReport;
  try {
    const outcome = await applyReplacePlan(
      plan,
      context.gateway ?? workspaceReplaceDesktop,
    );
    report = {
      message: `Replaced ${counted(outcome.matchCount, "match", "matches")} in ${counted(outcome.fileCount, "file", "files")}.`,
      problems: [...plan.problems, ...outcome.problems],
    };
  } catch (error) {
    report = { message: `Couldn't replace: ${errorText(error)}`, problems: [] };
  }
  update(context, { step: { kind: "idle" }, report });
  context.onReplaced();
}

/** The Replace row under the Search query. */
export function ReplaceRow({ context }: { context: ReplaceContext }) {
  const { replacement, preserveCase, step } = current(context);
  const { search, projectId, files } = context;
  const searching = search.phase === "searching";
  return (
    <div className="workspace-search__query-row">
      <span aria-hidden="true" />
      <div className="workspace-search__input">
        <input
          type="text"
          aria-label="Replace text"
          maxLength={2048}
          placeholder={search.useRegex ? "Replace ($1 for a group)" : "Replace"}
          value={replacement}
          disabled={!projectId}
          autoFocus
          onChange={(event) =>
            update(context, { replacement: event.target.value })
          }
        />
        <span className="workspace-search__query-actions">
          <button
            type="button"
            aria-label="Preserve case"
            title="Preserve case"
            aria-pressed={preserveCase}
            disabled={!projectId}
            onClick={() => update(context, { preserveCase: !preserveCase })}
          >
            <span aria-hidden="true">AB</span>
          </button>
          <button
            type="button"
            aria-label="Replace all"
            title="Replace all"
            disabled={
              !projectId ||
              searching ||
              search.phase !== "results" ||
              !files.length ||
              step.kind !== "idle"
            }
            onClick={() => void start(context, files)}
          >
            <ReplaceAll size={16} aria-hidden="true" />
          </button>
        </span>
      </div>
    </div>
  );
}

/** Replaces every match in one File. */
export function ReplaceFileAction({
  context,
  resource,
}: {
  context: ReplaceContext;
  resource: ResourceRef;
}) {
  return (
    <button
      className="workspace-replace__action"
      type="button"
      aria-label={`Replace all in ${resource.relativePath}`}
      title="Replace all in this file"
      disabled={current(context).step.kind !== "idle"}
      onClick={() => void start(context, [resource])}
    >
      <ReplaceAll size={14} aria-hidden="true" />
    </button>
  );
}

/** Replaces the one match Search reported. */
export function ReplaceMatchAction({
  context,
  match,
}: {
  context: ReplaceContext;
  match: WorkspaceSearchMatch;
}) {
  const { resource, lineNumber, columnNumber } = match;
  return (
    <button
      className="workspace-replace__action"
      type="button"
      aria-label={`Replace the match at line ${lineNumber}, column ${columnNumber} in ${resource.relativePath}`}
      title="Replace this match"
      disabled={current(context).step.kind !== "idle"}
      onClick={() =>
        void start(context, [resource], { lineNumber, columnNumber })
      }
    >
      <Replace size={14} aria-hidden="true" />
    </button>
  );
}

/** A preview being prepared, what the last Replace did, and the preview
 * awaiting a confirm. */
export function ReplaceStatus({
  context,
  rootName,
}: {
  context: ReplaceContext;
  rootName: (rootId: string) => string;
}) {
  const { step, report } = current(context);
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
      {step.kind === "confirming" || step.kind === "applying" ? (
        <WorkspaceReplaceDialog
          plan={step.plan}
          applying={step.kind === "applying"}
          rootName={rootName}
          onConfirm={() => void confirm(context, step.plan)}
          onCancel={() => {
            if (current(context).step.kind === "confirming")
              update(context, { step: { kind: "idle" } });
          }}
        />
      ) : null}
    </>
  );
}
