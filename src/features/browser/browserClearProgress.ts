import { create } from "zustand";

interface ClearProgress {
  busy: boolean;
  error: string | null;
  success: string | null;
}

/**
 * One clear at a time for the whole app, kept outside the component: a clear
 * started from the browser menu or Settings keeps running when its control
 * unmounts, and every control shown later reflects it instead of offering a
 * second clear that Rust would refuse.
 */
export const useClearProgress = create<ClearProgress>(() => ({
  busy: false,
  error: null,
  success: null,
}));

export function resetBrowserClearDataForTests() {
  useClearProgress.setState({ busy: false, error: null, success: null });
}
