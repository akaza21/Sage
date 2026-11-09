/** The File name a Search result heading shows for `relativePath`. */
export function searchResultFileName(relativePath: string): string {
  const normalized = relativePath.replaceAll("\\", "/").trim();
  if (!normalized) {
    return "";
  }
  const slash = normalized.lastIndexOf("/");
  return slash < 0 ? normalized : normalized.slice(slash + 1);
}
