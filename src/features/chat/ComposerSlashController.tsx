import type { ComponentProps } from "react";
import { lazyParts } from "./lazyParts";
import type * as Runtime from "./ComposerCommandRuntime";
const loadRuntime = () => import("./ComposerCommandRuntime");
const RuntimePart =
  lazyParts<Pick<typeof Runtime, "ComposerSlashController">>(loadRuntime);

// Both field mounting and typed sends enter through this lightweight facade.
// eslint-disable-next-line react-refresh/only-export-components -- the lazy facade exposes the chunk's command action
export async function slashCommandTurn(
  ...args: Parameters<typeof Runtime.slashCommandTurn>
) {
  return (await loadRuntime()).slashCommandTurn(...args);
}

export function ComposerSlashController(
  props: ComponentProps<typeof Runtime.ComposerSlashController>,
) {
  return <RuntimePart part="ComposerSlashController" {...props} />;
}
