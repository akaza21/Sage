import { create } from "zustand";

/** Bounded like the saved answers themselves (`MAX_SITE_PERMISSIONS`). */
const MAX_SITES = 256;

interface SiteReloadFailures {
  /**
   * Sites whose tabs did not reload after an Allow ended; their pages may
   * keep access until they reload. Shared by Settings and every tab drawer,
   * so closing a drawer or leaving Settings does not drop the warning.
   */
  sites: readonly string[];
  /** Adds newly failed sites; earlier ones stay until a reload confirms them. */
  add: (sites: readonly string[]) => void;
  /** Replaces the list with what a Reload tabs attempt still reports. */
  replace: (sites: readonly string[]) => void;
}

export const useSiteReloadFailures = create<SiteReloadFailures>((set) => ({
  sites: [],
  add: (added) => {
    if (added.length === 0) return;
    set((state) => ({
      sites: [...new Set([...state.sites, ...added])].slice(0, MAX_SITES),
    }));
  },
  replace: (sites) => set({ sites: [...new Set(sites)].slice(0, MAX_SITES) }),
}));

export function resetSiteReloadFailuresForTests() {
  useSiteReloadFailures.setState({ sites: [] });
}
