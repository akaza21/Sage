import { acpProjectionStoreFor } from "../../../src/app/acpProjectionRegistry";
import { useSessionAccessModeStore } from "../../../src/app/sessionAccessModeStore";
import { useWorkbenchStore } from "../../../src/app/store";

/**
 * Presents the rendered chat as a Claude Code session whose negotiated ACP
 * capabilities do (or do not) include `promptCapabilities.image`, and answers
 * the one native upload command a paste makes with a valid image reference.
 */
export function installComposerImageFixture(promptImages: boolean) {
  const store = useWorkbenchStore.getState();
  useSessionAccessModeStore.setState({ ready: true });
  useWorkbenchStore.setState({
    harnesses: store.harnesses.map((harness) => ({
      ...harness,
      name: "Claude Code",
      source: "builtIn" as const,
    })),
  });
  for (const tab of store.tabs.filter(
    (candidate) => candidate.kind === "chat",
  )) {
    const projection = acpProjectionStoreFor(tab.id).getState();
    if (!projection.page) continue;
    acpProjectionStoreFor(tab.id).setState({
      page: {
        ...projection.page,
        sessions: projection.page.sessions.map((session) => ({
          ...session,
          capabilities: { ...session.capabilities, promptImages },
        })),
      },
    });
  }
  let uploads = 0;
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    value: {
      transformCallback: () => 0,
      invoke: async (
        command: string,
        args: { request?: { label?: string; dataBase64?: string } },
      ) => {
        if (command !== "upload_chat_image") return null;
        uploads += 1;
        const bytes = atob(args.request?.dataBase64 ?? "").length;
        return {
          kind: "image",
          artifactId: `5a1f0c3e-2b4d-4c6e-8f10-${String(uploads).padStart(12, "0")}`,
          mediaType: "image/png",
          width: 320,
          height: 200,
          byteLength: bytes,
          sha256: "b".repeat(64),
          label: args.request?.label ?? "screenshot.png",
        };
      },
    },
  });
}
