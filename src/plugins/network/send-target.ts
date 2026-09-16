export function isNetworkSendTargetVisible(
  params: Record<string, unknown> | undefined,
): boolean {
  const transport = params?.transport as string | undefined;
  return (transport === "tcp" || transport === "udp") && params?.role === "server";
}

export function canSyncNetworkSendTarget(
  state: string | undefined,
  params: Record<string, unknown> | undefined,
): boolean {
  return state === "connected" && isNetworkSendTargetVisible(params);
}
