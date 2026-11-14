import { createContext } from "react";

// The runtime never commits a terminal Turn while a permission is pending (the
// pump tears the session down instead).
export const WorkedTurnTerminal = createContext(false);
