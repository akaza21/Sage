import { useState } from "react";

import type { ResourceRef } from "../../domain/workbench";
import type { WorkspaceSearchState } from "./useWorkspaceSearch";
import type {
  MatchPosition,
  ReplaceFileProblem,
  ReplacePlan,
  WorkspaceReplaceGateway,
} from "./workspaceReplace";

/** What previews and applies a Replace, loaded on the first one. */
async function replacer(gateway: WorkspaceReplaceGateway | undefined) {
  const plan = await import("./workspaceReplacePlan");
  return { plan, gateway: gateway ?? plan.workspaceReplaceDesktop };
}

export type ReplaceStep =
  | { kind: "idle" }
  | { kind: "planning" }
  | { kind: "confirming" | "applying"; plan: ReplacePlan };

export interface ReplaceReport {
  message: string;
  problems: ReplaceFileProblem[];
}

function errorText(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  return message.replace(/^invalid request: /, "");
}

function counted(count: number, word: string, plural: string) {
  return `${count} ${count === 1 ? word : plural}`;
}

/**
 * The Search panel's Replace: what to replace with, the preview awaiting a
 * confirm, and what the last Replace did. A Replace previews first and
 * changes nothing until `confirm`; afterwards `onReplaced` runs Search again.
 */
export function useWorkspaceReplace({
  gateway,
  search,
  onReplaced,
}: {
  /** The native gateway when left out. */
  gateway?: WorkspaceReplaceGateway;
  search: WorkspaceSearchState;
  onReplaced: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [replacement, setReplacement] = useState("");
  const [preserveCase, setPreserveCase] = useState(false);
  const [step, setStep] = useState<ReplaceStep>({ kind: "idle" });
  const [report, setReport] = useState<ReplaceReport | null>(null);

  /** Previews a Replace of `resources`, or of the one match at `only`. */
  async function start(resources: ResourceRef[], only?: MatchPosition) {
    const response = search.response;
    if (!response || !resources.length || step.kind !== "idle") return;
    setReport(null);
    setStep({ kind: "planning" });
    try {
      const loaded = await replacer(gateway);
      const plan = await loaded.plan.planReplace({
        gateway: loaded.gateway,
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
      setStep({ kind: "confirming", plan });
    } catch (error) {
      setStep({ kind: "idle" });
      setReport({
        message: `Couldn't preview the Replace: ${errorText(error)}`,
        problems: [],
      });
    }
  }

  async function confirm(plan: ReplacePlan) {
    setStep({ kind: "applying", plan });
    try {
      const loaded = await replacer(gateway);
      const outcome = await loaded.plan.applyReplacePlan(plan, loaded.gateway);
      setReport({
        message: `Replaced ${counted(outcome.matchCount, "match", "matches")} in ${counted(outcome.fileCount, "file", "files")}.`,
        problems: [...plan.problems, ...outcome.problems],
      });
    } catch (error) {
      setReport({
        message: `Couldn't replace: ${errorText(error)}`,
        problems: [],
      });
    }
    setStep({ kind: "idle" });
    onReplaced();
  }

  function cancel() {
    if (step.kind === "confirming") setStep({ kind: "idle" });
  }

  return {
    open,
    setOpen,
    replacement,
    setReplacement,
    preserveCase,
    setPreserveCase,
    step,
    idle: step.kind === "idle",
    report,
    start,
    confirm,
    cancel,
  };
}

export type WorkspaceReplace = ReturnType<typeof useWorkspaceReplace>;
