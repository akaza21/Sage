import { create } from "zustand";

interface ClearProgress {
  busy: boolean;
  error: string | null;
  success: string | null;
  /** The one mounted control that shows the outcome; null when none is. */
  owner: string | null;
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
  owner: null,
}));

const mounted = new Set<string>();

/** A control that mounts while nobody shows the outcome takes it over. */
export function mountClearControl(id: string) {
  mounted.add(id);
  if (useClearProgress.getState().owner === null)
    useClearProgress.setState({ owner: id });
  return () => {
    mounted.delete(id);
    if (useClearProgress.getState().owner !== id) return;
    const [next] = mounted;
    useClearProgress.setState({ owner: next ?? null });
  };
}

export function clearControlMounted() {
  return mounted.size > 0;
}

export function resetBrowserClearDataForTests() {
  useClearProgress.setState({
    busy: false,
    error: null,
    success: null,
    owner: null,
  });
  mounted.clear();
}
