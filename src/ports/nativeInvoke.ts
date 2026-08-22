export function isTauriRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export async function invokeNative<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}
