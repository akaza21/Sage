import type { AttachedContext } from "../../domain/attachedContext";
import { utf8ByteLength } from "../../domain/text";
import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";

interface ComposerAvailability {
  actionBusy: boolean;
  activeTurn: boolean;
  availableBinding: boolean;
  canPrompt: boolean;
  chatLoadFailed: boolean;
  harnessAvailable: boolean;
  harnessDiscoveryPhase: HarnessDiscoveryPhase;
  projectTransitioning: boolean;
  selectedHarness: boolean;
  useNativeAcp: boolean;
}

export function composerDisabledReason({
  actionBusy,
  activeTurn,
  availableBinding,
  canPrompt,
  chatLoadFailed,
  harnessAvailable,
  harnessDiscoveryPhase,
  projectTransitioning,
  selectedHarness,
  useNativeAcp,
}: ComposerAvailability): string | null {
  if (projectTransitioning) return "Wait for the Project to finish loading.";
  if (actionBusy) return "Wait for the current action to finish.";
  if (activeTurn)
    return "Wait for the active Turn to finish, or cancel it when available.";
  if (!selectedHarness) return "Choose an available Harness before sending.";
  if (harnessDiscoveryPhase === "checking")
    return "Wait while Sage checks the selected Harness.";
  if (["signInRequired", "consent", "waiting"].includes(harnessDiscoveryPhase))
    return "Finish signing in before sending.";
  if (!harnessAvailable) return "The selected Harness is unavailable.";
  if (useNativeAcp && !availableBinding)
    return "No reviewed local binding is available for this Harness.";
  if (chatLoadFailed)
    return "This Chat failed to load. Retry to send a prompt.";
  if (!canPrompt)
    return "The selected Harness cannot accept prompts in this session.";
  return null;
}

/** The limit Rust puts on the prompt it dispatches, attached context included. */
export const MAX_EFFECTIVE_PROMPT_BYTES = 100_000;
// Drafts come from the store; tolerate a field an older draft lacks.
const quoted = (value: string | undefined) =>
  utf8ByteLength(JSON.stringify(value ?? ""));
const plain = (value: string | undefined) => utf8ByteLength(value ?? "");

// Per draft (its context array keeps its identity until it changes): repeat
// calls in one render, and renders that change neither its text nor its
// context, never re-measure.
const measured = new WeakMap<
  readonly AttachedContext[],
  [string, string | null]
>();

/**
 * Why this draft cannot be sent because of its rendered size, or null.
 * Memoised: measured once until its text or context changes.
 */
export function sizeReason(
  text: string,
  contexts: readonly AttachedContext[],
): string | null {
  const known = measured.get(contexts);
  if (known?.[0] === text) return known[1];
  const bytes = effectivePromptBytes(text, contexts);
  const reason =
    bytes > MAX_EFFECTIVE_PROMPT_BYTES
      ? `This message and its attached context are too large to send (${Math.ceil(bytes / 1000)} of ${MAX_EFFECTIVE_PROMPT_BYTES / 1000} KB). Remove an annotation or shorten a comment.`
      : null;
  measured.set(contexts, [text, reason]);
  return reason;
}

/**
 * An upper bound on the UTF-8 size of the prompt Rust renders from this
 * draft (acp/attached_context.rs effective_prompt), so the composer can
 * refuse an oversized send before dispatch with a clear reason.
 */
export function effectivePromptBytes(
  text: string,
  contexts: readonly AttachedContext[],
): number {
  let bytes = utf8ByteLength(text);
  let annotations = 0;
  for (const context of contexts) {
    if (context.kind === "transcriptQuote") {
      if (annotations++ === 0) bytes += 96;
      bytes += 96 + quoted(context.text) + quoted(context.comment);
    } else if (context.kind === "diffLineComment")
      bytes += 128 + plain(context.diffRef?.relativePath) + plain(context.note);
    else if (context.kind === "browserPage")
      bytes +=
        160 +
        quoted(context.url) +
        quoted(context.title) +
        quoted(context.text);
  }
  return bytes;
}
