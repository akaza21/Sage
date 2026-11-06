// Test-only transport: requests come from the Rust command layer unchanged.
import { Session } from "node:inspector/promises";
import { createInterface } from "node:readline";

const session = new Session();
session.connect();
try {
  for await (const line of createInterface({ input: process.stdin })) {
    const { method, params } = JSON.parse(line);
    try {
      // Node has V8 but no DOM: expose the fixture button as an AX node.
      const result =
        method === "Accessibility.getFullAXTree"
          ? {
              nodes: [
                {
                  nodeId: "button",
                  backendDOMNodeId: 10,
                  role: { value: "button" },
                  name: { value: globalThis.button.textContent },
                },
              ],
            }
          : await session.post(method, params);
      process.stdout.write(JSON.stringify({ result }) + "\n");
    } catch (error) {
      process.stdout.write(JSON.stringify({ error: error.message }) + "\n");
    }
  }
} finally {
  session.disconnect();
}
