import type { ComponentProps } from "react";
import { lazyParts } from "./lazyParts";
import type * as Runtime from "./ComposerCommandRuntime";
const RuntimePart = lazyParts<typeof Runtime>(
  () => import("./ComposerCommandRuntime"),
);
export function ComposerSlashController(
  props: ComponentProps<typeof Runtime.ComposerSlashController>,
) {
  return <RuntimePart part="ComposerSlashController" {...props} />;
}
