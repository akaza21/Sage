import { createContext } from "react";

/**
 * The transcript's Copy announcer, so earlier and inherited answers announce
 * Copy exactly as the latest answer's footer does.
 */
export const AnswerCopyFeedback = createContext<
  ((text: string) => void) | undefined
>(undefined);
