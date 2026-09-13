export function environmentForGateStep(name, environment = process.env) {
  if (name !== "vitest") return environment;
  return {
    ...environment,
    VITEST_MAX_WORKERS: environment.VITEST_MAX_WORKERS || "1",
    VITEST_MIN_WORKERS: environment.VITEST_MIN_WORKERS || "1",
  };
}
