import type * as SuggestionsChunk from "../../ui/TextFieldSuggestions";
import { lazyParts } from "./lazyParts";

export const SuggestionsPart = lazyParts<typeof SuggestionsChunk>(
  () => import("../../ui/TextFieldSuggestions"),
);
SuggestionsPart.preload().catch(() => {});
