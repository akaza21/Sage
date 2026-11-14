// Loaded only when the user opens Replace, so Search alone carries none of it.

import type { Dispatch, SetStateAction } from "react";

import { utf8ByteLength } from "../../domain/text";
import type { ResourceRef } from "../../domain/workbench";
import { LoaderCircle, Replace, ReplaceAll } from "../../ui/leanIcons";
import type { WorkspaceSearchState } from "./useWorkspaceSearch";
import type {
  MatchPosition,
  ReplaceFileProblem,
  ReplacePlan,
  WorkspaceReplaceGateway,
} from "./workspaceReplace";
import type * as ApplyParts from "./workspaceReplaceApply";
import { planReplace } from "./workspaceReplacePlan";
import { workspaceReplaceDesktop } from "./workspaceReplaceDesktop";
import type { WorkspaceSearchMatch } from "./workspaceSearchDomain";
import { WORKSPACE_SEARCH_LIMITS } from "./workspaceSearchLimits";
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

export interface ReplaceContext {
  state: ReplaceState | undefined;
  setState: Dispatch<SetStateAction<ReplaceState | undefined>>;
  search: WorkspaceSearchState;
  gateway?: WorkspaceReplaceGateway;
  onReplaced: () => void;
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

function replacementProblem(replacement: string): string | null {
  return utf8ByteLength(replacement) > WORKSPACE_SEARCH_LIMITS.replacementBytes
    ? `The replacement is longer than ${WORKSPACE_SEARCH_LIMITS.replacementBytes} bytes.`
    : null;
}

const MAX_REPLACE_FILES =
  WORKSPACE_SEARCH_LIMITS.results * WORKSPACE_SEARCH_LIMITS.continuationPages;

function counted(count: number, word: string, plural: string) {
  return `${count} ${count === 1 ? word : plural}`;
}

// The confirm dialog and what applies a Replace load with its first
// preview, so neither weighs on the Replace row.
let applyParts: typeof ApplyParts | undefined;
const loadApply = () =>
  import("./workspaceReplaceApply").then((parts) => (applyParts = parts));

async function start(
  context: ReplaceContext,
  resources: ResourceRef[],
  only?: MatchPosition & { line: string },
) {
  const { search } = context;
  const { replacement, preserveCase, step } = current(context);
  const response = search.response;
  if (!response || !resources.length || step.kind !== "idle") return;
  const refused =
    replacementProblem(replacement) ??
    (resources.length > MAX_REPLACE_FILES
      ? `Replace changes at most ${MAX_REPLACE_FILES} files at once.`
      : null);
  if (refused) {
    update(context, { report: { message: refused, problems: [] } });
    return;
  }
  update(context, { report: null, step: { kind: "planning" } });
  try {
    const [plan] = await Promise.all([
      planReplace({
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
        ...(only
          ? {
              only: {
                lineNumber: only.lineNumber,
                columnNumber: only.columnNumber,
              },
              onlyLine: only.line,
            }
          : {}),
      }),
      loadApply(),
    ]);
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
    const { applyReplacePlan } = await loadApply();
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

export function ReplaceRow({ context }: { context: ReplaceContext }) {
  const { replacement, preserveCase, step } = current(context);
  const { search, projectId, files } = context;
  const searching = search.phase === "searching";
  const problem = replacementProblem(replacement);
  return (
    <div className="workspace-search__query-row">
      <span aria-hidden="true" />
      <div className="workspace-search__input">
        <input
          type="text"
          aria-label="Replace text"
          maxLength={2048}
          placeholder={search.useRegex ? "Replace ($1 for a group)" : "Replace"}
          title={
            search.useRegex
              ? "$1 or ${name} puts in a group. \\n and \\t stay as typed, and a pattern that matches empty text replaces nothing."
              : undefined
          }
          aria-invalid={problem ? true : undefined}
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
              !!problem ||
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

export function ReplaceMatchAction({
  context,
  match,
}: {
  context: ReplaceContext;
  match: WorkspaceSearchMatch;
}) {
  const { resource, lineNumber, columnNumber, preview } = match;
  return (
    <button
      className="workspace-replace__action"
      type="button"
      aria-label={`Replace the match at line ${lineNumber}, column ${columnNumber} in ${resource.relativePath}`}
      title="Replace this match"
      disabled={current(context).step.kind !== "idle"}
      onClick={() =>
        void start(context, [resource], {
          lineNumber,
          columnNumber,
          line: preview,
        })
      }
    >
      <Replace size={14} aria-hidden="true" />
    </button>
  );
}

export function ReplaceStatus({
  context,
  rootName,
}: {
  context: ReplaceContext;
  rootName: (rootId: string) => string;
}) {
  const { step, report } = current(context);
  const Dialog = applyParts?.WorkspaceReplaceDialog;
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
      {Dialog && (step.kind === "confirming" || step.kind === "applying") ? (
        <Dialog
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
