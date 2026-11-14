import { createGutterWorkerCore, type GutterRequest } from "./gutterWorkerCore";

const core = createGutterWorkerCore();

self.onmessage = (event: MessageEvent<GutterRequest>) => {
  const result = core.handle(event.data);
  if (result) self.postMessage(result);
};
