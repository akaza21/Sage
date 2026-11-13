import { createGutterWorkerCore, type GutterRequest } from "./gutterWorkerCore";

// The gutter's diff runs here, off the typing path. One worker serves
// every open editor; the page stops it when the last one closes.
const core = createGutterWorkerCore();

self.onmessage = (event: MessageEvent<GutterRequest>) => {
  const result = core.handle(event.data);
  if (result) self.postMessage(result);
};
