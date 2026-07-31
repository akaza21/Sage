import fs from "node:fs";

const [portValue, scenario, outputPath] = process.argv.slice(2);
const port = Number(portValue);
if (!Number.isInteger(port) || port < 1024 || port > 65535) {
  throw new Error("a bounded loopback debugging port is required");
}
if (!new Set(["chat", "explorer"]).has(scenario) || !outputPath) {
  throw new Error(
    "usage: prepare-sage-scenario.mjs <port> <chat|explorer> <output>",
  );
}

const deadline = Date.now() + 10_000;
let targets = [];
let pages = [];
while (Date.now() < deadline) {
  try {
    const response = await fetch(`http://127.0.0.1:${port}/json`);
    if (response.ok) targets = await response.json();
  } catch {
    // The exact setup process may still be starting its scoped endpoint.
  }
  pages = targets.filter(
    (target) =>
      target.type === "page" &&
      target.title === "Sage" &&
      target.url === "http://tauri.localhost/",
  );
  if (pages.length === 1) break;
  await new Promise((resolve) => setTimeout(resolve, 100));
}
if (pages.length !== 1) {
  throw new Error("expected exactly one Sage tauri.localhost setup page");
}

const socket = new WebSocket(pages[0].webSocketDebuggerUrl);
await Promise.race([
  new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve, { once: true });
    socket.addEventListener("error", reject, { once: true });
  }),
  new Promise((_, reject) =>
    setTimeout(() => reject(new Error("setup WebSocket timed out")), 10_000),
  ),
]);
let nextId = 0;
const pending = new Map();
socket.addEventListener("message", (event) => {
  const message = JSON.parse(event.data);
  const waiter = pending.get(message.id);
  if (!waiter) return;
  pending.delete(message.id);
  if (message.error) waiter.reject(new Error("CDP command failed"));
  else waiter.resolve(message.result);
});
function call(method, params = {}) {
  const id = ++nextId;
  const response = new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
  });
  socket.send(JSON.stringify({ id, method, params }));
  return Promise.race([
    response,
    new Promise((_, reject) =>
      setTimeout(
        () => reject(new Error("setup CDP command timed out")),
        10_000,
      ),
    ),
  ]);
}
async function evaluate(expression) {
  const result = await call("Runtime.evaluate", {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  if (result.exceptionDetails) throw new Error("setup page evaluation failed");
  return result.result.value;
}
async function waitFor(expression) {
  const end = Date.now() + 10_000;
  while (Date.now() < end) {
    if (await evaluate(expression)) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error("semantic setup state did not appear");
}

const chatSelected = await evaluate(`(()=>{
  const chats=[...document.querySelectorAll('.project-chat:not(.project-chat--utility)')];
  if(chats.length<1)return false;
  chats[0].click();
  return true;
})()`);
if (!chatSelected)
  throw new Error("no semantic project Chat control was available");
await waitFor(
  `document.querySelectorAll('.chat-surface[aria-label="Agent chat"]').length===1`,
);

let explorerSelected = false;
if (scenario === "explorer") {
  explorerSelected = await evaluate(`(()=>{
    const buttons=[...document.querySelectorAll('button[aria-label="Explorer"]')];
    if(buttons.length!==1)return false;
    buttons[0].click();
    return true;
  })()`);
  if (!explorerSelected)
    throw new Error("Explorer semantic control was ambiguous");
  await waitFor(
    `document.querySelectorAll('.tree[aria-label="Project files"]').length===1&&document.querySelectorAll('button[aria-label="Explorer"].activity-rail__button--active').length===1`,
  );
}

const finalState = await evaluate(`(()=>({
  chatCenterCount:document.querySelectorAll('.chat-surface[aria-label="Agent chat"]').length,
  explorerSidebarCount:document.querySelectorAll('.tree[aria-label="Project files"]').length,
  activeExplorerCount:document.querySelectorAll('button[aria-label="Explorer"].activity-rail__button--active').length
}))()`);
const verified =
  finalState.chatCenterCount === 1 &&
  (scenario === "chat" ||
    (explorerSelected &&
      finalState.explorerSidebarCount === 1 &&
      finalState.activeExplorerCount === 1));
if (!verified) throw new Error("semantic setup verification failed");

const result = {
  schemaVersion: 1,
  scenario,
  executed: true,
  semanticVerified: true,
  title: pages[0].title,
  origin: pages[0].url,
  targetId: pages[0].id,
  chatCenterCount: finalState.chatCenterCount,
  explorerSidebarCount: finalState.explorerSidebarCount,
  activeExplorerCount: finalState.activeExplorerCount,
};
fs.writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, {
  encoding: "utf8",
  flag: "wx",
});
socket.close();
