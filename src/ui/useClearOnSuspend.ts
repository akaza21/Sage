import { useEffect } from "react";

/**
 * Runs `clear` whenever the component leaves the screen: when a React
 * Activity boundary above it hides, as Settings does while the window is
 * minimized, and when it unmounts. Hidden trees keep their state and DOM
 * until wake, so anything that holds a secret, shown or typed, or waits on
 * the user, clears itself here rather than surviving out of sight. `clear`
 * must be stable, or it also runs on every change of identity.
 */
export function useClearOnSuspend(clear: () => void) {
  useEffect(() => clear, [clear]);
}
