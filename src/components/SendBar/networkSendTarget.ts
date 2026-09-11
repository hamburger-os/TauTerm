/** TCP server pseudo-target understood by the backend network router. */
export const ALL_NETWORK_PEERS = "__all__";

/** Only TCP/UDP server roles expose a mutable send target. */
export function isTargetBarVisible(params: Record<string, unknown> | undefined): boolean {
  const transport = params?.transport as string | undefined;
  const role = params?.role as string | undefined;
  return (transport === "tcp" || transport === "udp") && role === "server";
}

/**
 * Runtime target synchronization is valid only after the network session is connected.
 * Saved/disconnected/connecting sessions have no backend NetworkSideChannel yet and must
 * keep their target selection purely in frontend session state until the runtime exists.
 */
export function canSyncNetworkSendTarget(
  pluginId: string | undefined,
  connectionState: string | undefined,
  params: Record<string, unknown> | undefined,
): boolean {
  return pluginId === "network"
    && connectionState === "connected"
    && isTargetBarVisible(params);
}
