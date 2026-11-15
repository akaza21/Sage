import { findMatches, type FindRequest } from "./inFileFind";

self.onmessage = (event: MessageEvent<FindRequest>) => {
  self.postMessage(findMatches(event.data));
};
