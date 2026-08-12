import { useWorkbenchStore } from "../../app/store";

export function HarnessDiscoveryNotice() {
  const phase = useWorkbenchStore((state) => state.harnessDiscoveryPhase);
  const error = useWorkbenchStore((state) => state.harnessDiscoveryError);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const retry = useWorkbenchStore((state) => state.retryHarnessDiscovery);
  const requestSignIn = useWorkbenchStore(
    (state) => state.requestHarnessSignIn,
  );
  const confirmSignIn = useWorkbenchStore(
    (state) => state.confirmHarnessSignIn,
  );
  const cancelSignIn = useWorkbenchStore((state) => state.cancelHarnessSignIn);
  const logout = useWorkbenchStore((state) => state.logoutHarness);
  const accountManaged = useWorkbenchStore((state) => {
    const harness = state.harnesses.find(
      (candidate) => candidate.id === state.selectedHarnessId,
    );
    return harness?.transport === "acp" && harness.accountModelDiscovery;
  });

  if (!accountManaged) return null;

  if (phase === "checking") {
    return (
      <div className="composer__notice" role="status">
        Checking Codex account and available models…
      </div>
    );
  }
  if (phase === "signInRequired") {
    return (
      <div className="composer__notice" role="status">
        Sign in to ChatGPT to load models for this harness.
        <button
          className="button button--compact"
          type="button"
          disabled={transitioning}
          onClick={requestSignIn}
        >
          Sign in
        </button>
      </div>
    );
  }
  if (phase === "consent") {
    return (
      <div className="composer__notice" role="status">
        Sage will open the official OpenAI sign-in page in your browser. Codex
        stores credentials; Sage does not receive or retain them.
        <button
          className="button button--compact"
          type="button"
          disabled={transitioning}
          onClick={() => void confirmSignIn()}
        >
          Continue
        </button>
        <button
          className="button button--compact"
          type="button"
          disabled={transitioning}
          onClick={() => void cancelSignIn()}
        >
          Cancel
        </button>
      </div>
    );
  }
  if (phase === "waiting") {
    return (
      <div className="composer__notice" role="status">
        Finish signing in through your browser.
        <button
          className="button button--compact"
          type="button"
          disabled={transitioning}
          onClick={() => void cancelSignIn()}
        >
          Cancel
        </button>
      </div>
    );
  }
  if (phase === "ready") {
    return (
      <div className="composer__notice composer__notice--account">
        ChatGPT account connected.
        <button
          className="button button--compact"
          type="button"
          disabled={transitioning}
          onClick={() => void logout()}
        >
          Sign out
        </button>
      </div>
    );
  }
  if (phase === "error") {
    return (
      <div className="composer__error" role="alert">
        {error ?? "Harness discovery failed safely."}
        <button
          className="button button--compact"
          type="button"
          disabled={transitioning}
          onClick={() => void retry()}
        >
          Retry
        </button>
      </div>
    );
  }
  return null;
}
