function formatHostPort(host: string, port: number): string {
  const trimmedHost = host.trim();
  const displayHost = trimmedHost.includes(":")
    && !(trimmedHost.startsWith("[") && trimmedHost.endsWith("]"))
    ? `[${trimmedHost}]`
    : trimmedHost;
  return `${displayHost}:${port}`;
}

/** Telnet is currently client-only; the target belongs on the dynamic second line. */
export function telnetSessionTitle(): string {
  return "Telnet @ Client";
}

export function telnetEndpointLabel(
  params: Record<string, unknown>,
  endpoint: string,
): string {
  const host = typeof params.host === "string" && params.host.trim()
    ? params.host.trim()
    : endpoint.trim();
  const port = typeof params.port === "number" && Number.isInteger(params.port)
    && params.port > 0 && params.port <= 65535
    ? params.port
    : 23;
  return formatHostPort(host, port);
}
