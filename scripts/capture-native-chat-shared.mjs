export const IDENTIFIER = /^[A-Za-z0-9_-]{1,128}$/u;

export function retainCaptureError(primary, next) {
  if (!primary) return next;
  if (next?.cleanupError && !primary.cleanupError)
    primary.cleanupError = next.cleanupError;
  return primary;
}

export function boundedEvaluate(cdp, deadline) {
  return (expression) =>
    cdp.evaluate(expression, Math.min(deadline, Date.now() + 5000));
}
