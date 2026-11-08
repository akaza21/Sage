/** One usage limit window, as a harness reports it. */
export interface StatusUsageWindow {
  label: string | null;
  usedPercent: number;
  windowMinutes: number | null;
}

/**
 * What `/status` shows, from data the chat already holds. Account, plan
 * and usage appear only when the harness reports them.
 */
export interface SessionStatusInput {
  model: { model: string; effort: string | null } | null;
  access: string | null | undefined;
  account?: string | null;
  usage?: { plan: string | null; windows: readonly StatusUsageWindow[] } | null;
}

function windowLabel({ label, windowMinutes }: StatusUsageWindow): string {
  if (label) return label;
  if (windowMinutes === null) return "Usage";
  return windowMinutes % 1440 === 0
    ? `${windowMinutes / 1440}-day limit`
    : windowMinutes % 60 === 0
      ? `${windowMinutes / 60}-hour limit`
      : `${windowMinutes}-minute limit`;
}

/** The panel's rows, in order; absent values are left out. */
export function sessionStatusRows({
  model,
  access,
  account,
  usage,
}: SessionStatusInput): [string, string][] {
  const rows: [string, string | null | undefined][] = [
    ["Model", model?.model],
    ["Effort", model?.effort],
    ["Access", access],
    ["Account", account],
    ["Plan", usage?.plan],
    ...(usage?.windows ?? []).map((window): [string, string] => [
      windowLabel(window),
      `${Math.round(window.usedPercent)}% used`,
    ]),
  ];
  return rows.filter((row): row is [string, string] => Boolean(row[1]));
}
