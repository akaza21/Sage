const FOLLOW_END_THRESHOLD_PX = 64;

export function followEndScrollBehavior(): ScrollBehavior {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches
    ? "auto"
    : "smooth";
}

export function transcriptIsAtEnd(
  transcript: Pick<HTMLElement, "clientHeight" | "scrollHeight" | "scrollTop">,
): boolean {
  const distance =
    transcript.scrollHeight - transcript.scrollTop - transcript.clientHeight;
  return distance <= FOLLOW_END_THRESHOLD_PX;
}
