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
  if (projectTransitioning) return "Wait for the project to finish loading.";
  if (actionBusy) return "Wait for the current action to finish.";
  if (activeTurn)
    return "Wait for the active turn to finish, or cancel it when available.";
  if (!selectedHarness) return "Choose an available harness before sending.";
  if (harnessDiscoveryPhase === "checking")
    return "Wait while Sage checks the selected harness.";
  if (["signInRequired", "consent", "waiting"].includes(harnessDiscoveryPhase))
    return "Finish signing in before sending.";
  if (!harnessAvailable) return "The selected harness is unavailable.";
  if (useNativeAcp && !availableBinding)
    return "No reviewed local binding is available for this harness.";
  if (chatLoadFailed)
    return "This chat failed to load. Retry to send a prompt.";
  if (!canPrompt)
    return "The selected harness cannot accept prompts in this session.";
  return null;
}

/** Rust prompt cap includes attached context. */
export const MAX_EFFECTIVE_PROMPT_BYTES = 100_000;
// Tolerate missing fields in older drafts.
const quoted = (value: string | undefined) =>
  utf8ByteLength(JSON.stringify(value ?? ""));
const plain = (value: string | undefined) => utf8ByteLength(value ?? "");

// Cache per draft text/context identity; don't remeasure unchanged renders.
const measured = new WeakMap<
  readonly AttachedContext[],
  [string, string | null]
>();

/** Size refusal reason, or null; memoised by text/context. */
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

/** UTF-8 upper bound of Rust's acp/attached_context.rs effective_prompt; reject oversize before dispatch. */
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
