import { lazyParts } from "../features/chat/lazyParts";
import type * as ResourcesChunk from "./AgentTaskResources";
import type * as MenuChunk from "./AgentHeaderMenu";

export const AgentTaskResourcesPart = lazyParts<typeof ResourcesChunk>(() =>
  import("./AgentTaskResources").then(async (module) => {
    await module.AgentTaskResources.preload();
    return module;
  }),
);

export const AgentHeaderMenuPart = lazyParts<typeof MenuChunk>(
  () => import("./AgentHeaderMenu"),
);
// Once loaded anywhere, every header mounts its menu at once, as it always
// did. A header mounts the menu only then, so a failed load never reaches
// the render and the next hover, focus or open asks again.
let menuReady = false;
const menuListeners = new Set<() => void>();
export const agentHeaderMenuReady = () => menuReady;
export function subscribeAgentHeaderMenu(listener: () => void) {
  menuListeners.add(listener);
  return () => void menuListeners.delete(listener);
}
export const preloadAgentHeaderMenu = () =>
  AgentHeaderMenuPart.preload().then(() => {
    if (menuReady) return;
    menuReady = true;
    for (const listener of menuListeners) listener();
  });
