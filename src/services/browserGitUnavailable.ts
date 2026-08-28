export class BrowserGitMutationUnavailableError extends Error {
  override name = "BrowserGitMutationUnavailableError";
}

function nativeGitUnavailable(
  message: string,
): (_request: unknown, _expectedTransitionEpoch: number) => Promise<never> {
  return () => Promise.reject(new BrowserGitMutationUnavailableError(message));
}

export const browserGitUnavailable = {
  abortGitInProgress: nativeGitUnavailable(
    "Git abort requires the native Sage desktop",
  ),
  mutateGitFile: nativeGitUnavailable(
    "Git stage, unstage, discard, and conflict resolution require the native Sage desktop",
  ),
  commitGitRepository: nativeGitUnavailable(
    "Git commits require the native Sage desktop",
  ),
  checkoutGitLocalBranch: nativeGitUnavailable(
    "Git checkout requires the native Sage desktop",
  ),
  continueGitCherryPick: nativeGitUnavailable(
    "Continue cherry-pick requires the native Sage desktop",
  ),
  continueGitRebase: nativeGitUnavailable(
    "Continue rebase requires the native Sage desktop",
  ),
  continueGitRevert: nativeGitUnavailable(
    "Continue revert requires the native Sage desktop",
  ),
  createGitLocalBranch: nativeGitUnavailable(
    "Git create branch requires the native Sage desktop",
  ),
  deleteGitLocalBranch: nativeGitUnavailable(
    "Git delete branch requires the native Sage desktop",
  ),
  fetchGitRemote: nativeGitUnavailable(
    "Git fetch requires the native Sage desktop",
  ),
  pushGitRemote: nativeGitUnavailable(
    "Git push requires the native Sage desktop",
  ),
  renameGitLocalBranch: nativeGitUnavailable(
    "Git rename branch requires the native Sage desktop",
  ),
};
