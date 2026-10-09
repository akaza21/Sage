import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import {
  parsePromotionArgs,
  promoteCapture,
} from "./capture-native-chat-evidence.mjs";

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href
) {
  try {
    const workspace = path.resolve(
      path.dirname(fileURLToPath(import.meta.url)),
      "..",
    );
    console.log(
      JSON.stringify(
        promoteCapture(workspace, parsePromotionArgs(process.argv.slice(2))),
      ),
    );
  } catch (error) {
    const reason = /^[a-z0-9-]{1,80}$/u.test(error.message)
      ? error.message
      : "promotion-operation-blocked";
    console.error("native-chat-promotion-blocked: " + reason);
    process.exitCode = 1;
  }
}
