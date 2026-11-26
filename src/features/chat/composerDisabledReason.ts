import type { AttachedContext } from "../../domain/attachedContext";
import type { AgentSessionProjection } from "../../domain/acp";
import { utf8ByteLength } from "../../domain/text";
import {
  harnessUnavailableReason,
  type HarnessDefinition,
} from "../../domain/harness";
import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";

interface ComposerAvailability {
  actionBusy: boolean;
  activeTurn: boolean;
  availableBinding: boolean;
  chatLoadFailed: boolean;
  harnessAvailable: boolean;
  harnessDiscoveryPhase: HarnessDiscoveryPhase;
  projectTransitioning: boolean;
  selectedHarness?: Pick<HarnessDefinition, "name"> | null;
  useNativeAcp: boolean;
}

/** Discovery phases that wait for the user to finish signing in. */
export const SIGN_IN_PHASES: readonly HarnessDiscoveryPhase[] = [
  "signInRequired",
  "consent",
  "waiting",
];

/**
 * A session whose start was abandoned before the harness answered (a slow
 * first launch) is back to created and cold with no capabilities yet; the
 * next send starts it again, so it does not refuse prompts.
 */
export function startsOnSend(
  session: Pick<
    AgentSessionProjection,
    "state" | "residency" | "nextRuntimeEpoch"
  >,
): boolean {
  return (
    session.state === "created" &&
    session.residency === "cold" &&
    session.nextRuntimeEpoch !== null
  );
}

/** The session's own refusal, given when no reason below applies. */
export const SESSION_PROMPT_REFUSAL =
  "The selected harness cannot accept prompts in this session.";

/** Why the composer is refused before the session's own refusal applies. */
export function composerDisabledReason({
  actionBusy,
  activeTurn,
  availableBinding,
  chatLoadFailed,
  harnessAvailable,
  harnessDiscoveryPhase,
  projectTransitioning,
  selectedHarness,
  useNativeAcp,
}: ComposerAvailability): string | null {
  if (projectTransitioning) return "Wait for the project to finish loading.";
  if (actionBusy) return "Wait for the current action to finish.";
  if (activeTurn) return "Wait for the active turn to finish.";
  if (!selectedHarness) return "Choose an available harness before sending.";
  if (harnessDiscoveryPhase === "checking")
    return "Wait while Sage checks the selected harness.";
  if (SIGN_IN_PHASES.includes(harnessDiscoveryPhase))
    return "Finish signing in before sending.";
  if (!harnessAvailable) return `${selectedHarness.name} is unavailable.`;
  if (useNativeAcp && !availableBinding)
    return harnessUnavailableReason("adapterAbsent", selectedHarness.name);
  if (useNativeAcp && chatLoadFailed)
    return "This chat failed to load. Retry to send a prompt.";
  return null;
}

/** Rust prompt cap includes attached context. */
export const MAX_EFFECTIVE_PROMPT_BYTES = 100_000;
// Tolerate missing fields in older drafts.
const quoted = (value: string | undefined) =>
  utf8ByteLength(JSON.stringify(value ?? ""));
const plain = (value: string | undefined) => utf8ByteLength(value ?? "");

const measured = new WeakMap<
  readonly AttachedContext[],
  [string, string | null]
>();

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
