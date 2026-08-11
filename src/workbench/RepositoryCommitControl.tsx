import { useEffect, useId, useRef, useState, type FormEvent } from "react";

import { useWorkbenchStore } from "../app/store";
import {
  gitCommitMessageError,
  hasStagedChanges,
  MAX_GIT_COMMIT_MESSAGE_BYTES,
} from "../domain/gitCommit";
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
  const clearError = useWorkbenchStore(
    (state) => state.clearRepositoryCommitError,
  );
  const error = useWorkbenchStore((state) =>
    state.repositoryCommitError?.rootId === repository.rootId
      ? state.repositoryCommitError
      : null,
  );
  const staged = hasStagedChanges(repository);
  const editorUnavailable = repository.state !== "repository";
  const repositoryUnavailable = editorUnavailable || repository.truncated;
  const validationError = message ? gitCommitMessageError(message) : null;
  const commitDisabled =
    controlsDisabled ||
    submitting ||
    repositoryUnavailable ||
    !staged ||
    gitCommitMessageError(message) !== null;
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

  return (
    <form
      className="repository-commit"
      aria-label={`Commit staged changes in ${rootLabel}`}
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
          disabled={controlsDisabled || submitting || editorUnavailable}
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
            : staged
              ? "Only staged changes will be committed. Ctrl/Command+Enter to commit."
              : "Stage changes to enable commit."}
        </span>
        <button
          type="submit"
          aria-label={`Commit staged changes in ${rootLabel}`}
          disabled={commitDisabled}
        >
          {submitting ? "Committing…" : "Commit"}
        </button>
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
          aria-label={`${rootLabel} commit failed`}
        >
          {error.message}
        </p>
      ) : null}
    </form>
  );
}
