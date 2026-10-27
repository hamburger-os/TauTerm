import type { ConnectionStatus } from "../core/session-lifecycle";
import type { IconName } from "../components/common/Icon";

const icons: Record<ConnectionStatus, IconName> = {
  connected: "status-connected",
  disconnected: "status-disconnected",
  connecting: "status-connecting",
  disconnecting: "status-connecting",
};

export function statusIconName(state: ConnectionStatus): IconName {
  return icons[state];
}

export function statusValue(state: ConnectionStatus): string {
  return `statusBar.${state}`;
}
