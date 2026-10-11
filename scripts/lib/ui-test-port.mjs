export function resolveUiPort(environment = process.env) {
  const value = environment.SAGE_UI_PORT;
  if (value === undefined) return 1431;
  const port = Number(value);
  if (
    typeof value !== "string" ||
    !/^[0-9]{1,5}$/u.test(value) ||
    !Number.isInteger(port) ||
    port < 1024 ||
    port > 65535
  ) {
    throw new Error(
      "SAGE_UI_PORT must be a decimal integer between 1024 and 65535",
    );
  }
  return port;
}
