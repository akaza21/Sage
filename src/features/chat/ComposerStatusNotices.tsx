import type { ChatComposerRegionProps } from "./chatComposerRegionProps";
import { ComposerOptionsPart } from "./lazyParts";
import type { ReactNode } from "react";
import type { HarnessDefinition } from "../../domain/harness";
import { Notice } from "../../ui/Notice";
import type { NoticeSeverity } from "../../ui/Notice";

export type ComposerStatusNoticeProps = Pick<
  ChatComposerRegionProps,
  "workbenchError" | "chatError" | "chatAuth"
> & {
  imageStatus: ComposerProblem | null;
  signInHarness?: Pick<HarnessDefinition, "name" | "signIn"> | null;
  authRecovery?: {
    harnessId: string;
    scope: string;
    receivedAt?: number | null;
    onSignedIn: () => void;
  };
};

/** Errors remain available while the optional notice surface loads. */
export function ComposerNoticeFrame(
  props: ComposerStatusNoticeProps & { children?: ReactNode },
) {
  const { workbenchError, chatError, imageStatus, children } = props;
  const problems = resolveComposerProblems(
    [imageStatus, workbenchError, chatError].filter(
      (problem): problem is ComposerProblem => !!problem,
    ),
  );
  return problems.length || children ? (
    <div
      className="composer__notices"
      role="region"
      aria-label="Composer notices"
      tabIndex={0}
    >
      {children}
      {problems.map(({ identity, message, severity }) => (
        <Notice
          key={identity}
          identity={identity}
          message={message}
          severity={severity}
          progress={severity === "info"}
        />
      ))}
    </div>
  ) : null;
}

export function ComposerStatusNotices(props: ComposerStatusNoticeProps) {
  const refusal =
    props.authRecovery || props.chatAuth?.errorKind === "signInRequired";
  const frame = <ComposerNoticeFrame {...props} />;
  return refusal ? (
    <ComposerOptionsPart
      part="ComposerNoticeSurface"
      {...props}
      fallback={frame}
    />
  ) : (
    frame
  );
}

export interface ComposerProblem {
  /** Canonical identity built from problem kind and owning resource scope. */
  identity: string;
  subject: "send" | "attachment";
  message: ReactNode;
  severity: NoticeSeverity;
}

export function composerProblem(
  kind: string,
  scope:
    | string
    | readonly [string | null | undefined, string | number | null | undefined],
  message: ReactNode,
  subject: ComposerProblem["subject"] = "send",
  severity: NoticeSeverity = "blocking",
): ComposerProblem | null {
  return message
    ? { identity: JSON.stringify([kind, scope]), message, subject, severity }
    : null;
}

/** The nearest owner wins; equal copy on different resources stays separate. */
function resolveComposerProblems(problems: readonly ComposerProblem[]) {
  const resolved = new Map<string, ComposerProblem>();
  for (const problem of problems) {
    const id = problem.identity;
    const previous = resolved.get(id);
    if (!previous) resolved.set(id, problem);
    else
      resolved.set(id, {
        ...(problem.subject === "attachment" ? problem : previous),
        // Blocking wins; incoming information cannot downgrade a problem.
        severity:
          previous.severity === "blocking" || problem.severity === "info"
            ? previous.severity
            : problem.severity,
      });
  }
  return [...resolved.values()];
}
