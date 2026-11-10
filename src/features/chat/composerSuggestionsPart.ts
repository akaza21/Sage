import type * as SuggestionsChunk from "../../ui/TextFieldSuggestions";
import { lazyParts } from "./lazyParts";

/**
 * The suggestion listbox the composer's `/` and `@` menus share, in a lazy
 * chunk of its own so neither menu's chunk carries it. It starts loading
 * with whichever menu's chunk loads first, so a menu's first opening finds
 * it ready; a failed load is retried when a menu renders it.
 */
export const SuggestionsPart = lazyParts<typeof SuggestionsChunk>(
  () => import("../../ui/TextFieldSuggestions"),
);
SuggestionsPart.preload().catch(() => {});
