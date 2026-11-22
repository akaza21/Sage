import type { RepositoryMutationError } from "../app/repositoryMutationSlice";
import { resourceKey } from "../domain/resourceIdentity";
import { Notice } from "../ui/Notice";

function mutationSentence(error: RepositoryMutationError): string {
  const path = error.resource.relativePath;
  if (error.kind === "containment") {
    return `Source Control containment blocked ${error.action} for ${path}.`;
  }
  if (error.kind === "effectUnknown") {
    return `Source Control could not confirm ${error.action} for ${path}.`;
  }
  return `Source Control could not ${error.action} ${path}.`;
}

/** The one explanation of a failed Git file action, beside its repository. */
export function RepositoryMutationNotice({
  error,
}: {
  error: RepositoryMutationError;
}) {
  return (
    <Notice
      identity={`repository-mutation:${resourceKey(error.resource)}:${error.action}`}
      severity="blocking"
      message={mutationSentence(error)}
      details={error.message}
    />
  );
}
