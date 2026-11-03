import { lazyParts } from "../features/chat/lazyParts";
import type * as NavigationChunk from "./AgentCollapsedNavigation";

export const AgentNavigationPart = lazyParts<typeof NavigationChunk>(
  () => import("./AgentCollapsedNavigation"),
);
