import { afterEach, expect, it } from "vitest";
import { useWorkbenchStore } from "../../app/store";
import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../../app/acpProjectionRegistry";
import { page, textItem } from "./acp/acpProjection.fixture";
import { retryChatTurn } from "./retryChatTurn";
import type { AttachedContext } from "../../domain/attachedContext";

const initial = useWorkbenchStore.getState();
afterEach(() => {
  useWorkbenchStore.setState(initial, true);
  clearAcpProjectionRegistry();
});
function failedRequest() {
  const saved = page("failed");
  saved.turns[0].items = [
    {
      ...textItem(),
      payload: { role: "user", text: "Inspect this image and note" },
    },
  ];
  saved.turns[0].attachedContext = [
    {
      kind: "image",
      artifactId: "00000000-0000-4000-8000-000000000000",
      mediaType: "image/png",
      width: 1,
      height: 1,
      byteLength: 1,
      sha256: "a".repeat(64),
      label: "sample.png",
    },
    {
      kind: "browserPage",
      tabId: "browser-1",
      url: "https://example.com/",
      title: "Example",
      text: "Heading",
    },
  ];
  useWorkbenchStore.setState({
    project: { id: "project-1", name: "P", roots: [] },
    projectTransitioning: false,
    activeTabId: "chat-1",
    tabs: [
      {
        kind: "chat",
        id: "chat-1",
        title: "Chat",
        position: 0,
        pinned: false,
        dirty: false,
      },
    ],
  });
  acpProjectionStoreFor(saved.chatId).setState({ page: saved });
  return saved;
}
it("delivers the same failed request with its saved image and browser context", async () => {
  const saved = failedRequest();
  const requests: { prompt: string; context: readonly AttachedContext[] }[] =
    [];
  await retryChatTurn(saved, async (prompt, context) => {
    requests.push({ prompt, context });
    return "applied";
  });
  expect(requests).toEqual([
    {
      prompt: "Inspect this image and note",
      context: saved.turns[0].attachedContext,
    },
  ]);
});
it("does not replay a request after the projected session changes", async () => {
  const saved = failedRequest();
  acpProjectionStoreFor(saved.chatId).setState({
    page: { ...saved, currentSessionId: "session-new" as never },
  });
  const delivered: string[] = [];
  await retryChatTurn(saved, async (prompt) => {
    delivered.push(prompt);
    return "applied";
  });
  expect(delivered).toEqual([]);
});
it("reports an unavailable artifact and never falls back to a text-only retry", async () => {
  const saved = failedRequest();
  const delivered: (readonly AttachedContext[])[] = [];
  await retryChatTurn(saved, async (_prompt, context) => {
    delivered.push(context);
    return "rejected";
  });
  expect(delivered).toEqual([saved.turns[0].attachedContext]);
  expect(
    useWorkbenchStore.getState().chatUiById[saved.chatId]?.error,
  ).toContain("Reattach unavailable items (sample.png, browserPage)");
});
