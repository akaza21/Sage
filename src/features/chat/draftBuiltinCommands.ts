import { useEffect, useState } from "react";
import { invokeNative, isTauriRuntime } from "../../ports/nativeInvoke";
import type { AvailableAcpCommand } from "../../services/acpCommandsDesktop";

/** As `MAX_DRAFT_BUILTINS` in the native command placement. */
const MAX_BUILTINS = 32;

/**
 * Insert-only rows for the draft's "Skills and commands" section: the
 * harness's curated built-in commands from installation metadata
 * (`list_builtin_commands`). Only the current harness's rows show; none
 * outside the desktop app or on failure.
 */
export function useDraftBuiltinCommands(harnessId: string | undefined) {
  const [shown, setShown] = useState<{
    harnessId?: string;
    rows: AvailableAcpCommand[];
  }>({ rows: [] });
  useEffect(() => {
    let live = !!harnessId && isTauriRuntime();
    if (live)
      invokeNative<unknown>("list_builtin_commands", { harnessId }).then(
        (names) =>
          live &&
          setShown({
            harnessId,
            rows: Array.isArray(names)
              ? names.slice(0, MAX_BUILTINS).map((name) => ({
                  name: String(name),
                  description: "Built-in",
                  inputHint: null,
                  source: "builtin",
                  placement: { section: "skillsAndCommands" },
                }))
              : [],
          }),
        () => live && setShown({ harnessId, rows: [] }),
      );
    return () => {
      live = false;
    };
  }, [harnessId]);
  return shown.harnessId === harnessId ? shown.rows : [];
}
