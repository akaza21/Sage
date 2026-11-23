import type { ChatComposerRegionProps } from "./chatComposerRegionProps";
import { NoticePart } from "./lazyParts";

/**
 * A harness's composer chrome. Its code shares the session strip's chunk,
 * which any chat with a session surface already loads.
 */
export const harnessChrome = (
  place: "above" | "below",
  props: ChatComposerRegionProps,
) =>
  props.chatAuth?.page?.sessionSurface ? (
    <NoticePart part="ComposerChrome" place={place} {...props} />
  ) : null;
