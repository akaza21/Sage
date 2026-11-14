import type { BrowserPageIdentity } from "./browserAddress";
import { useShowFullUrl } from "./browserPreferencesStore";

export type { BrowserPageIdentity };

/** Kept out of browserAddress.ts, which the tab strips load at startup. */
export function useAddressText(
  editing: boolean,
  page: BrowserPageIdentity | null,
  address: string,
): string {
  const showFullUrl = useShowFullUrl();
  if (editing || !page) return address;
  return showFullUrl ? page.url : new URL(page.url).host;
}
