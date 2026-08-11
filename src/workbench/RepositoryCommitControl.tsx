import { useEffect, useId, useRef, useState, type FormEvent } from "react";

import { useWorkbenchStore } from "../app/store";
import {
  hasStagedChanges,
  MAX_GIT_COMMIT_MESSAGE_BYTES,
  normalizeGitCommitMessage,
} from "../domain/gitCommit";
import type { RootRepositoryStatus } from "../domain/workbench";

interface RepositoryCommitControlProps {
  repository: RootRepositoryStatus;
  rootName: string;
  controlsDisabled: boolean;
  projectStatusTruncated: boolean;
}

function validMessage(message: string): boolean {
  try {
    normalizeGitCommitMessage(message);
    return true;
  } catch {
    return false;
  }
}

export function RepositoryCommitControl({
  repository,
  rootName,
  controlsDisabled,
  projectStatusTruncated,
}: RepositoryCommitControlProps) {
  const [message, setMessage] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const messageRef = useRef<HTMLTextAreaElement>(null);
  const focusAfterSubmit = useRef(false);
  const helpId = useId();
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
  const repositoryUnavailable =
    repository.state !== "repository" ||
    repository.truncated ||
    projectStatusTruncated;
  const commitDisabled =
    controlsDisabled ||
    submitting ||
    repositoryUnavailable ||
    !staged ||
    !validMessage(message);
  const description = error ? `${helpId} ${errorId}` : helpId;

  useEffect(() => {
    if (!submitting && focusAfterSubmit.current) {
      focusAfterSubmit.current = false;
      messageRef.current?.focus();
    }
  }, [submitting]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (commitDisabled) return;
    setSubmitting(true);
    const committed = await commitRepository(repository.rootId, message);
    focusAfterSubmit.current = true;
    setSubmitting(false);
    if (committed) setMessage("");
  };

  return (
    <form
      className="repository-commit"
      aria-label={`Commit staged changes in ${rootName}`}
      onSubmit={(event) => void submit(event)}
    >
      <label className="repository-commit__label">
        <span className="sr-only">Commit message for {rootName}</span>
        <textarea
          ref={messageRef}
          rows={2}
          maxLength={MAX_GIT_COMMIT_MESSAGE_BYTES}
          value={message}
          placeholder="Commit message"
          aria-describedby={description}
          aria-invalid={error ? "true" : undefined}
          disabled={controlsDisabled || submitting || repositoryUnavailable}
          onChange={(event) => {
            setMessage(event.target.value);
            clearError(repository.rootId);
          }}
        />
      </label>
      <div className="repository-commit__footer">
        <span id={helpId} className="repository-commit__help">
          {repositoryUnavailable
            ? "Repository status must be complete before committing."
            : staged
              ? "Only staged changes will be committed."
              : "Stage changes to enable commit."}
        </span>
        <button
          type="submit"
          aria-label={`Commit staged changes in ${rootName}`}
          disabled={commitDisabled}
        >
          {submitting ? "Committing…" : "Commit"}
        </button>
      </div>
      {error ? (
        <p
          id={errorId}
          className="repository-commit__error"
          role="alert"
          aria-label={`${rootName} commit failed`}
        >
          {error.message}
        </p>
      ) : null}
    </form>
  );
}
