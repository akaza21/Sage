import type { LanguageDiagnosticSeverity } from "../../domain/languageService";

export type ProblemCardKind = "Bug" | "Flag" | "Informational";

export function problemSeverityLabel(
  severity: LanguageDiagnosticSeverity,
): ProblemCardKind {
  switch (severity) {
    case "error":
      return "Bug";
    case "warning":
      return "Flag";
    default:
      return "Informational";
  }
}

export function problemSummaryTitle(input: {
  bugs: number;
  flags: number;
  infos: number;
}): string {
  if (input.bugs > 0) {
    return input.bugs === 1 ? "1 Bug" : `${input.bugs} Bugs`;
  }
  if (input.flags > 0) {
    return input.flags === 1 ? "1 Flag" : `${input.flags} Flags`;
  }
  if (input.infos > 0) {
    return input.infos === 1
      ? "1 Informational"
      : `${input.infos} Informational`;
  }
  return "No problems";
}

export function countProblemKinds(
  severities: readonly LanguageDiagnosticSeverity[],
): { bugs: number; flags: number; infos: number } {
  let bugs = 0;
  let flags = 0;
  let infos = 0;
  for (const severity of severities) {
    const kind = problemSeverityLabel(severity);
    if (kind === "Bug") bugs += 1;
    else if (kind === "Flag") flags += 1;
    else infos += 1;
  }
  return { bugs, flags, infos };
}
