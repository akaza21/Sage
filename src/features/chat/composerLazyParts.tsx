import type { ChatComposerRegionProps } from "./chatComposerRegionProps";
import type * as SlashChunk from "./ComposerSlashController";
import { lazyParts } from "./lazyParts";

/** The composer's small deferred facade: slash commands and harness chrome. */
export const SlashPart = lazyParts<
  Pick<typeof SlashChunk, "ComposerSlashController" | "ComposerChrome">
>(() => import("./ComposerSlashController"));

/** A harness's composer chrome, loaded only once a session has a surface. */
export const harnessChrome = (
  place: "above" | "below",
  props: ChatComposerRegionProps,
) =>
  props.chatAuth?.page?.sessionSurface ? (
    <SlashPart part="ComposerChrome" place={place} {...props} />
  ) : null;
