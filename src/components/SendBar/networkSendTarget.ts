/** TCP server pseudo-target understood by the backend network router. */
export const ALL_NETWORK_PEERS = "__all__";

/** Only TCP/UDP server roles expose a mutable send target. */
export function isTargetBarVisible(params: Record<string, unknown> | undefined): boolean {
  const transport = params?.transport as string | undefined;
  const role = params?.role as string | undefined;
  return (transport === "tcp" || transport === "udp") && role === "server";
}
