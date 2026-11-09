import { useId, useState, type RefObject } from "react";
import { useStore } from "zustand";
import "../../styles/composer-command-panel.css";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
import { useSessionAccessSelection } from "../../app/useSessionAccessSelection";
import type { AcpReviewTarget } from "../../domain/acp";
import { utf8ByteLength } from "../../domain/text";
import { MenuPopover } from "../../ui/MenuPopover";
import { isCurrentLocalBranch } from "../git/localBranches";
import { currentSession } from "./acp/acpTurnState";
import { chatActionController } from "./acp/useAcpChatActions";
import { confirmedComposerModel } from "./inheritedComposerModel";
import { sessionStatusRows } from "./sessionStatusRows";

/** Panels the composer's `/` commands open beside the field. */
export function ComposerCommandPanel({
  kind,
  boundChatId,
  fieldRef,
  onClose,
}: {
  kind: "review" | "status";
  boundChatId?: string;
  fieldRef: RefObject<HTMLTextAreaElement | null>;
  onClose: () => void;
}) {
  const id = useId();
  return (
    <MenuPopover
      id={id}
      role="dialog"
      ariaLabel={kind === "review" ? "Review" : "Session status"}
      className="composer__popover composer-command-panel"
      placement="above-start"
      open
      triggerRef={fieldRef}
      onDismiss={onClose}
    >
      {kind === "review" ? (
        <ReviewTargetForm
          onStart={(target) => {
            onClose();
            fieldRef.current?.focus();
            void chatActionController(boundChatId).runSessionAction({
              kind: "review",
              target,
            });
          }}
        />
      ) : (
        <SessionStatus boundChatId={boundChatId} />
      )}
    </MenuPopover>
  );
}

const TARGETS = [
  ["uncommittedChanges", "Uncommitted changes"],
  ["baseBranch", "Against a base branch"],
  ["commit", "A commit"],
  ["custom", "Custom instructions"],
] as const;

/** The one repository root's local branches and recent commits. */
function useRepository() {
  const roots = useWorkbenchStore((state) => state.repositories.roots);
  const repositories = roots.filter((root) => root.state === "repository");
  return repositories.length === 1 ? repositories[0]! : null;
}

/** The target the form describes, or null while it is incomplete. */
function reviewTarget(
  kind: AcpReviewTarget["kind"],
  fields: { branch: string; sha: string; title?: string; instructions: string },
): AcpReviewTarget | null {
  const { branch, sha, title, instructions } = fields;
  switch (kind) {
    case "uncommittedChanges":
      return { kind };
    case "baseBranch":
      return branch ? { kind, branch } : null;
    case "commit":
      return /^[0-9a-f]{4,64}$/iu.test(sha)
        ? {
            kind,
            sha,
            ...(title && utf8ByteLength(title) <= 512 ? { title } : {}),
          }
        : null;
    case "custom":
      return instructions && utf8ByteLength(instructions) <= 16384
        ? { kind, instructions }
        : null;
  }
}

function ReviewTargetForm({
  onStart,
}: {
  onStart: (target: AcpReviewTarget) => void;
}) {
  const id = useId();
  const repository = useRepository();
  const branches = (repository?.branches ?? []).filter(
    (name) => !isCurrentLocalBranch(name, repository?.branch),
  );
  const commits = repository?.recentCommits ?? [];
  const [kind, setKind] =
    useState<AcpReviewTarget["kind"]>("uncommittedChanges");
  const [picked, setBranch] = useState("");
  const [sha, setSha] = useState("");
  const [instructions, setInstructions] = useState("");
  // The branch list may change while the picker is open.
  const branch = branches.includes(picked) ? picked : (branches[0] ?? "");
  const commit = sha.trim();
  const title = commits.find((entry) =>
    entry.sha.startsWith(commit.toLowerCase()),
  )?.subject;
  const target = reviewTarget(kind, {
    branch,
    sha: commit,
    title,
    instructions: instructions.trim(),
  });
  return (
    <form
      className="composer-command-panel__form"
      onSubmit={(event) => {
        // The portal's events still bubble to the composer's own form.
        event.preventDefault();
        event.stopPropagation();
        if (target) onStart(target);
      }}
    >
      <fieldset>
        <legend>Review</legend>
        {TARGETS.map(([value, label]) => (
          <label key={value}>
            <input
              type="radio"
              name={id}
              value={value}
              checked={kind === value}
              disabled={value === "baseBranch" && branches.length === 0}
              onChange={() => setKind(value)}
            />
            {label}
          </label>
        ))}
      </fieldset>
      {kind === "baseBranch" ? (
        <select
          className="select"
          aria-label="Base branch"
          value={branch}
          onChange={(event) => setBranch(event.target.value)}
        >
          {branches.map((name) => (
            <option key={name}>{name}</option>
          ))}
        </select>
      ) : kind === "commit" ? (
        <>
          <input
            className="input"
            aria-label="Commit SHA"
            placeholder="Commit SHA"
            list={`${id}-commits`}
            value={sha}
            onChange={(event) => setSha(event.target.value)}
          />
          <datalist id={`${id}-commits`}>
            {commits.map((entry) => (
              <option key={entry.sha} value={entry.sha}>
                {entry.subject}
              </option>
            ))}
          </datalist>
        </>
      ) : kind === "custom" ? (
        <textarea
          className="input"
          aria-label="Review instructions"
          placeholder="What should the review focus on?"
          rows={3}
          value={instructions}
          onChange={(event) => setInstructions(event.target.value)}
        />
      ) : null}
      <button
        type="submit"
        className="button button--compact button--primary"
        disabled={!target}
      >
        Start review
      </button>
    </form>
  );
}

function SessionStatus({ boundChatId }: { boundChatId?: string }) {
  const page = useStore(
    acpProjectionStoreFor(boundChatId),
    (state) => state.page,
  );
  const session = page ? currentSession(page) : null;
  const harness = useWorkbenchStore((state) =>
    state.harnesses.find(
      (candidate) => candidate.bindingId === session?.bindingId,
    ),
  );
  const access = useSessionAccessSelection(
    harness?.bindingId,
    harness?.accessModes,
  );
  const rows = sessionStatusRows({
    model: confirmedComposerModel(page?.sessionConfig),
    access: access.candidates.find(({ id }) => id === access.effective)?.label,
  });
  return rows.length ? (
    <dl className="composer-command-panel__status">
      {rows.map(([term, value]) => (
        <div key={term}>
          <dt>{term}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  ) : (
    <p className="composer-command-panel__empty">
      Status appears once the session is running.
    </p>
  );
}
