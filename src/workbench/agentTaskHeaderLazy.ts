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
// Once loaded anywhere, every header mounts its menu at once, as it always did.
let menuReady = false;
export const agentHeaderMenuReady = () => menuReady;
export const preloadAgentHeaderMenu = () =>
  AgentHeaderMenuPart.preload().then(() => {
    menuReady = true;
  });
