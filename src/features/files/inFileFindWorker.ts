import { findMatches, type FindRequest } from "./inFileFind";

// Regex Find and Replace run here, so a slow pattern never blocks
// typing. The page terminates this worker when a search runs too long.
self.onmessage = (event: MessageEvent<FindRequest>) => {
  self.postMessage(findMatches(event.data));
};
