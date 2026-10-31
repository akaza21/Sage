import { invokeNative } from "./nativeInvoke";

/** Saved browser passwords; see `browser/passwords/commands.rs`. Replies are
 * admitted in `savedPasswords.ts`. Only Reveal returns a password. */
export function passwordsOverview(query: string): Promise<unknown> {
  return invokeNative("browser_passwords", { query });
}

export function passwordsImportCsv(): Promise<unknown> {
  return invokeNative("browser_passwords_import_csv");
}

export function passwordsDeleteImportedFile(): Promise<unknown> {
  return invokeNative("browser_passwords_delete_imported_file");
}

export function passwordsUpdate(
  id: string,
  username: string,
  password: string | null,
): Promise<unknown> {
  return invokeNative("browser_passwords_update", { id, username, password });
}

export function passwordsDelete(id: string): Promise<unknown> {
  return invokeNative("browser_passwords_delete", { id });
}

export function passwordsDeleteAll(): Promise<unknown> {
  return invokeNative("browser_passwords_delete_all");
}

export function passwordsReveal(id: string): Promise<unknown> {
  return invokeNative("browser_passwords_reveal", { id });
}

export function passwordsCopy(id: string): Promise<unknown> {
  return invokeNative("browser_passwords_copy", { id });
}
