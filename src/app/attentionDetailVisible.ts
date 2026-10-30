import { createContext, useContext } from "react";

// Whether the Agent chat detail is on screen (Agent mode, not Settings).
// Kept apart from the attention store so the always-loaded Workbench shell
// does not pull attention logic into its closure.
const DetailVisible = createContext(true);
export const AttentionDetailVisible = DetailVisible.Provider;
export const useAttentionDetailVisible = () => useContext(DetailVisible);
