import { create } from "zustand";

interface ClearProgress {
  busy: boolean;
  error: string | null;
  success: string | null;
}

/**
 * Kept outside the component so a clear keeps running when its control
 * unmounts, and later controls never offer a second clear that Rust would
 * refuse.
 */
export const useClearProgress = create<ClearProgress>(() => ({
  busy: false,
  error: null,
  success: null,
}));

export function resetBrowserClearDataForTests() {
  useClearProgress.setState({ busy: false, error: null, success: null });
}
