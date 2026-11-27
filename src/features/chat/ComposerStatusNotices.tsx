import type { ChatComposerRegionProps } from "./chatComposerRegionProps";
import { ComposerOptionsPart } from "./lazyParts";
import type { ReactNode } from "react";
import type { HarnessDefinition } from "../../domain/harness";
import { Notice } from "../../ui/Notice";
import type { NoticeSeverity } from "../../ui/Notice";
import { NoticeStack, Stacked, type StackEntry } from "../../ui/NoticeStack";

export type ComposerStatusNoticeProps = Partial<
  Pick<ChatComposerRegionProps, "workbenchError" | "chatError" | "chatAuth">
> & {
  imageStatus?: ComposerProblem | null;
  /** The composer's other notices, ahead of its status in source order. */
  notices?: readonly StackEntry[];
  signInHarness?: Pick<HarnessDefinition, "name" | "signIn"> | null;
  authRecovery?: {
    harnessId: string;
    scope: string;
    receivedAt?: number | null;
    onSignedIn: () => void;
  };
};

/** The composer's one notice stack; errors remain available while the
 * optional notice surface loads. */
export function ComposerNoticeFrame(
  props: ComposerStatusNoticeProps & { children?: ReactNode },
) {
  const {
    workbenchError,
    chatError,
    imageStatus,
    notices = [],
    children,
  } = props;
  return (
    <NoticeStack
      label="Composer notices"
      notices={[
        ...notices,
        !!children && (
          <Stacked key="sign-in" severity="blocking">
            {children}
          </Stacked>
        ),
        ...resolveComposerProblems([
          imageStatus,
          workbenchError,
          chatError,
        ]).map(({ identity, message, severity }) => (
          <Notice
            key={identity}
            identity={identity}
            message={message}
            severity={severity}
            progress={severity === "info"}
          />
        )),
      ]}
    />
  );
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
function resolveComposerProblems(
  problems: readonly (ComposerProblem | null | undefined)[],
) {
  const resolved = new Map<string, ComposerProblem>();
  for (const problem of problems) {
    if (!problem) continue;
    const id = problem.identity;
    const previous = resolved.get(id);
    resolved.set(
      id,
      previous
        ? {
            ...(problem.subject === "attachment" ? problem : previous),
            // Blocking wins; incoming information cannot downgrade a problem.
            severity:
              previous.severity === "blocking" || problem.severity === "info"
                ? previous.severity
                : problem.severity,
          }
        : problem,
    );
  }
  return [...resolved.values()];
}
