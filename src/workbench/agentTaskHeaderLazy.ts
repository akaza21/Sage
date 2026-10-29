import { lazyParts } from "../features/chat/lazyParts";
import type * as ResourcesChunk from "./AgentTaskResources";

export const AgentTaskResourcesPart = lazyParts<typeof ResourcesChunk>(() =>
  import("./AgentTaskResources").then(async (module) => {
    await module.AgentTaskResources.preload();
    return module;
  }),
);
