import { useEffect, useState } from "react";
import { invokeNative, isTauriRuntime } from "../../ports/nativeInvoke";
import {
  decodeCommandList,
  type AvailableAcpCommand,
} from "../../services/acpCommandsDesktop";

/**
 * The commands a new-chat draft offers for its harness before any session
 * (`list_draft_commands`): the harness's last reported list for this project,
 * else its latest from anywhere, else its curated built-ins. Commands that
 * need a started chat are left out natively. Bounded and validated like the
 * live list; `null` outside the desktop app or on failure.
 */
export async function readDraftCommands(
  harnessId: string,
  projectId: string | null,
): Promise<AvailableAcpCommand[] | null> {
  if (!isTauriRuntime()) return null;
  const rows = await invokeNative<unknown>("list_draft_commands", {
    harnessId,
    projectId,
  }).catch(() => null);
  return decodeCommandList({ revision: 1, commands: rows })?.commands ?? null;
}

/** Rows for the draft's menus; re-read when a session or probe of the harness
 * reports a changed list. Only the current harness's rows show. */
export function useDraftCommands(
  harnessId: string | undefined,
  projectId: string | null,
) {
  const [shown, setShown] = useState<{
    key?: string;
    rows: AvailableAcpCommand[];
  }>({ rows: [] });
  const key = harnessId && JSON.stringify([harnessId, projectId]);
  useEffect(() => {
    if (!harnessId || !key || !isTauriRuntime()) return;
    let live = true;
    let unlisten: (() => void) | undefined;
    const read = () =>
      void readDraftCommands(harnessId, projectId).then(
        (rows) => live && setShown({ key, rows: rows ?? [] }),
      );
    read();
    void import("@tauri-apps/api/event").then(({ listen }) =>
      listen<{ harnessId?: unknown }>("draft-commands-changed-v1", (event) => {
        if (event.payload?.harnessId === harnessId) read();
      }).then((stop) => (live ? (unlisten = stop) : stop())),
    );
    return () => {
      live = false;
      unlisten?.();
    };
  }, [harnessId, projectId, key]);
  return shown.key === key ? shown.rows : [];
}
