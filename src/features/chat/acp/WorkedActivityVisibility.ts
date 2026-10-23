import { createContext } from "react";

export const WorkedActivityVisibility = createContext(true);

/**
 * True inside a terminal Turn's Worked activity. The runtime never commits a
 * terminal Turn while a permission is pending (the pump tears the session
 * down instead), so a terminal Turn offers no permission decision.
 */
export const WorkedTurnTerminal = createContext(false);
