import { useEffect } from "react";

/**
 * Hidden Activity trees keep their state and DOM until wake, so anything
 * holding a secret clears itself here. `clear` must be stable, or it also runs
 * on every change of identity.
 */
export function useClearOnSuspend(clear: () => void) {
  useEffect(() => clear, [clear]);
}
