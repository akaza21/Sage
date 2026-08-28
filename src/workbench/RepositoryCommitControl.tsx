import { useEffect, useId, useRef, useState, type FormEvent } from "react";

import { useWorkbenchStore } from "../app/store";
import {
  canAbortGitSequence,
  canCommitRepository,
  canCompleteMerge,
  canContinueCherryPick,
  canContinueRebase,
  canContinueRevert,
  exclusiveGitSequence,
  gitCommitMessageError,
  MAX_GIT_COMMIT_MESSAGE_BYTES,
} from "../domain/gitCommit";
import { isConflictedChange } from "../domain/gitMutation";
import type { RootRepositoryStatus } from "../domain/workbench";

interface RepositoryCommitControlProps {
  repository: RootRepositoryStatus;
  rootLabel: string;
  controlsDisabled: boolean;
}

export function RepositoryCommitControl({
  repository,
  rootLabel,
  controlsDisabled,
}: RepositoryCommitControlProps) {
  const [message, setMessage] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const messageRef = useRef<HTMLTextAreaElement>(null);
  const focusAfterSubmit = useRef(false);
  const helpId = useId();
  const validationId = useId();
  const errorId = useId();
  const commitRepository = useWorkbenchStore((state) => state.commitRepository);
  const continueCherryPick = useWorkbenchStore(
    (state) => state.continueCherryPick,
  );
  const continueRebase = useWorkbenchStore((state) => state.continueRebase);
  const continueRevert = useWorkbenchStore((state) => state.continueRevert);
  const abortInProgress = useWorkbenchStore((state) => state.abortInProgress);
  const clearError = useWorkbenchStore(
    (state) => state.clearRepositoryCommitError,
  );
  const error = useWorkbenchStore((state) =>
    state.repositoryCommitError?.rootId === repository.rootId
      ? state.repositoryCommitError
      : null,
  );
  const completingMerge = canCompleteMerge(repository);
  const historyFlags = [
    repository.rebaseInProgress === true ? "rebase" : null,
    repository.cherryPickInProgress === true ? "cherryPick" : null,
    repository.revertInProgress === true ? "revert" : null,
  ].filter(
    (value): value is "rebase" | "cherryPick" | "revert" => value !== null,
  );
  const historyContinue = historyFlags.length === 1 ? historyFlags[0] : null;
  const historyContinuable =
    historyContinue === "rebase"
      ? canContinueRebase(repository)
      : historyContinue === "cherryPick"
        ? canContinueCherryPick(repository)
        : historyContinue === "revert"
          ? canContinueRevert(repository)
          : false;
  const committable = canCommitRepository(repository);
  const hasConflicts = repository.changes.some(isConflictedChange);
  const editorUnavailable = repository.state !== "repository";
  const repositoryUnavailable = editorUnavailable || repository.truncated;
  const validationError = message ? gitCommitMessageError(message) : null;
  const commitDisabled =
    controlsDisabled ||
    submitting ||
    repositoryUnavailable ||
    historyContinue !== null ||
    !committable ||
    gitCommitMessageError(message) !== null;
  const continueDisabled =
    controlsDisabled ||
    submitting ||
    repositoryUnavailable ||
    !historyContinuable;
  const sequence = exclusiveGitSequence(repository);
  const abortable = canAbortGitSequence(repository);
  const abortDisabled =
    controlsDisabled || submitting || repositoryUnavailable || !abortable;
  const abortAction =
    sequence === "cherryPick"
      ? `Abort cherry-pick in ${rootLabel}`
      : sequence === "revert"
        ? `Abort revert in ${rootLabel}`
        : sequence === "merge"
          ? `Abort merge in ${rootLabel}`
          : `Abort rebase in ${rootLabel}`;
  const commitAction = completingMerge
    ? `Commit merge in ${rootLabel}`
    : `Commit staged changes in ${rootLabel}`;
  const continueAction =
    historyContinue === "cherryPick"
      ? `Continue cherry-pick in ${rootLabel}`
      : historyContinue === "revert"
        ? `Continue revert in ${rootLabel}`
        : `Continue rebase in ${rootLabel}`;
  const description = [
    helpId,
    validationError ? validationId : null,
    error ? errorId : null,
  ]
    .filter(Boolean)
    .join(" ");

  useEffect(() => {
    if (!submitting && focusAfterSubmit.current) {
      focusAfterSubmit.current = false;
      messageRef.current?.focus();
    }
  }, [submitting]);

  const submitCommit = async () => {
    if (commitDisabled) return;
    setSubmitting(true);
    const committed = await commitRepository(repository.rootId, message);
    focusAfterSubmit.current = true;
    setSubmitting(false);
    if (committed) setMessage("");
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    void submitCommit();
  };

  const submitAbort = async () => {
    if (abortDisabled) return;
    setSubmitting(true);
    await abortInProgress(repository.rootId);
    focusAfterSubmit.current = true;
    setSubmitting(false);
  };

  const submitHistoryContinue = async () => {
    if (continueDisabled) return;
    setSubmitting(true);
    if (historyContinue === "cherryPick") {
      await continueCherryPick(repository.rootId);
    } else if (historyContinue === "revert") {
      await continueRevert(repository.rootId);
    } else {
      await continueRebase(repository.rootId);
    }
    focusAfterSubmit.current = true;
    setSubmitting(false);
  };

  return (
    <form
      className="repository-commit"
      aria-label={historyContinue ? continueAction : commitAction}
      onSubmit={submit}
    >
      <label className="repository-commit__label">
        <span className="sr-only">Commit message for {rootLabel}</span>
        <textarea
          ref={messageRef}
          rows={2}
          maxLength={MAX_GIT_COMMIT_MESSAGE_BYTES}
          value={message}
          placeholder="Commit message"
          aria-describedby={description}
          aria-invalid={validationError || error ? "true" : undefined}
          disabled={
            controlsDisabled ||
            submitting ||
            editorUnavailable ||
            historyContinue !== null
          }
          onChange={(event) => {
            setMessage(event.target.value);
            clearError(repository.rootId);
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
              event.preventDefault();
              void submitCommit();
            }
          }}
        />
      </label>
      <div className="repository-commit__footer">
        <span id={helpId} className="repository-commit__help">
          {repositoryUnavailable
            ? "Repository status must be complete before committing."
            : historyContinue && hasConflicts
              ? historyContinue === "cherryPick"
                ? "Resolve remaining conflicts to continue cherry-pick."
                : historyContinue === "revert"
                  ? "Resolve remaining conflicts to continue revert."
                  : "Resolve remaining conflicts to continue rebase."
              : historyContinue === "cherryPick"
                ? "This continues the in-progress cherry-pick."
                : historyContinue === "revert"
                  ? "This continues the in-progress revert."
                  : historyContinue === "rebase"
                    ? "This continues the in-progress rebase."
                    : completingMerge
                      ? "This completes the in-progress merge."
                      : hasConflicts && repository.mergeInProgress
                        ? "Resolve remaining conflicts to enable commit."
                        : committable
                          ? "Only staged changes will be committed. Ctrl/Command+Enter to commit."
                          : "Stage changes to enable commit."}
        </span>
        <span className="repository-commit__actions">
          {abortable ? (
            <button
              type="button"
              aria-label={abortAction}
              disabled={abortDisabled}
              onClick={() => void submitAbort()}
            >
              {submitting
                ? sequence === "cherryPick"
                  ? "Aborting cherry-pick…"
                  : sequence === "revert"
                    ? "Aborting revert…"
                    : sequence === "merge"
                      ? "Aborting merge…"
                      : "Aborting rebase…"
                : sequence === "cherryPick"
                  ? "Abort cherry-pick"
                  : sequence === "revert"
                    ? "Abort revert"
                    : sequence === "merge"
                      ? "Abort merge"
                      : "Abort rebase"}
            </button>
          ) : null}
          {historyContinue ? (
            <button
              type="button"
              aria-label={continueAction}
              disabled={continueDisabled}
              onClick={() => void submitHistoryContinue()}
            >
              {submitting
                ? historyContinue === "cherryPick"
                  ? "Continuing cherry-pick…"
                  : historyContinue === "revert"
                    ? "Continuing revert…"
                    : "Continuing rebase…"
                : historyContinue === "cherryPick"
                  ? "Continue cherry-pick"
                  : historyContinue === "revert"
                    ? "Continue revert"
                    : "Continue rebase"}
            </button>
          ) : (
            <button
              type="submit"
              aria-label={commitAction}
              disabled={commitDisabled}
            >
              {submitting
                ? "Committing…"
                : completingMerge
                  ? "Commit merge"
                  : "Commit"}
            </button>
          )}
        </span>
      </div>
      {validationError ? (
        <p
          id={validationId}
          className="repository-commit__error"
          role="alert"
          aria-label={`${rootLabel} commit message invalid`}
        >
          {validationError}
        </p>
      ) : null}
      {error ? (
        <p
          id={errorId}
          className="repository-commit__error"
          role="alert"
          aria-label={
            historyContinue === "cherryPick"
              ? `${rootLabel} continue cherry-pick failed`
              : historyContinue === "revert"
                ? `${rootLabel} continue revert failed`
                : historyContinue === "rebase"
                  ? `${rootLabel} continue rebase failed`
                  : `${rootLabel} commit failed`
          }
        >
          {error.message}
        </p>
      ) : null}
    </form>
  );
}
