import { lazyParts } from "../chat/lazyParts";
import type * as HarnessSignInChunk from "./HarnessSignInStatus";

export const HarnessSignInPart = lazyParts<typeof HarnessSignInChunk>(
  () => import("./HarnessSignInStatus"),
);
