export const MAX_BREADCRUMB_PATH_LENGTH = 4096;
export const MAX_BREADCRUMB_SEGMENTS = 6;

export function fileBreadcrumbSegments(path: string): string[] {
  if (path.length === 0 || path.length > MAX_BREADCRUMB_PATH_LENGTH) {
    return [];
  }
  const segments = path.split(/[\\/]/).filter((segment) => segment.length > 0);
  if (segments.length <= MAX_BREADCRUMB_SEGMENTS) {
    return segments;
  }
  return [segments[0]!, "…", ...segments.slice(-(MAX_BREADCRUMB_SEGMENTS - 2))];
}
