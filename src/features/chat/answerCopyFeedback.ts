import { createContext } from "react";

export const AnswerCopyFeedback = createContext<
  ((text: string) => void) | undefined
>(undefined);
