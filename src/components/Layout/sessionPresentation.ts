import type { NetworkPeerEntry, TabInfo } from "../../context/SessionContext";
import { pluginRegistry } from "../../core/plugin-registry";

export interface SessionPresentationLabels {
  trdpCapture: string;
  trdpUnconfigured: string;
  trdpDisabled: string;
}

export interface SessionPresentationNetworkState {
  networkPeers: Record<string, NetworkPeerEntry[]>;
  networkLocalAddrs: Record<string, string>;
}

function formatHostPort(host: string, port: number): string {
  const trimmedHost = host.trim();
  const displayHost = trimmedHost.includes(":")
    && !(trimmedHost.startsWith("[") && trimmedHost.endsWith("]"))
    ? `[${trimmedHost}]`
    : trimmedHost;
  return `${displayHost}:${port}`;
}

function getNetworkClientLocalAddr(
  tab: TabInfo,
  networkState?: SessionPresentationNetworkState,
): string | undefined {
  if (tab.pluginId !== "network" || !networkState) return undefined;
  const params = (tab.params ?? {}) as Record<string, unknown>;
  if (((params.role as string | undefined) ?? "client") !== "client") return undefined;

  return params.transport === "udp"
    ? networkState.networkLocalAddrs[tab.id]
    : networkState.networkPeers[tab.id]?.[0]?.localAddr;
}

/**
 * Canonical second-line identity used by Session cards and Pane headers.
 * Root sessions prefer the plugin-owned dynamic summary; built-in special cases remain for
 * runtime-aware presentations (SSH IPv6 formatting, TRDP localized labels, network local addr).
 * Child terminal rows intentionally keep their runtime endpoint, matching the Sidebar child row.
 */
export function getSessionSubtitle(
  tab: TabInfo,
  labels: SessionPresentationLabels,
  networkState?: SessionPresentationNetworkState,
): string {
  if (tab.parentId) return tab.endpoint;

  const params = (tab.params ?? {}) as Record<string, unknown>;
  const pluginSubtitle = pluginRegistry
    .get(tab.pluginId)
    ?.sessionPresentation
    ?.subtitle?.(params, tab.endpoint)
    ?.trim();
  let subtitle: string;

  if (pluginSubtitle) {
    subtitle = pluginSubtitle;
  } else if (tab.pluginId === "ssh") {
    const host = typeof params.host === "string" && params.host.trim()
      ? params.host.trim()
      : tab.endpoint;
    const port = typeof params.port === "number" && Number.isInteger(params.port)
      && params.port > 0 && params.port <= 65535
      ? params.port
      : 22;
    subtitle = formatHostPort(host, port);
  } else if (tab.pluginId === "iperf") {
    const listenIp = typeof params.listen_ip === "string" && params.listen_ip.trim()
      ? params.listen_ip.trim()
      : "0.0.0.0";
    const listenPort = typeof params.listen_port === "number" && Number.isFinite(params.listen_port)
      ? params.listen_port
      : (params.version === "iperf3" ? 5201 : 5001);
    subtitle = `${listenIp}:${listenPort}`;
  } else if (tab.pluginId === "trdp") {
    const mode = params.mode === "monitor" ? "monitor" : "node";
    if (mode === "monitor") {
      const interfaceA = typeof params.capture_interface === "string"
        ? params.capture_interface.trim()
        : "";
      const interfaceB = typeof params.capture_interface_b === "string"
        ? params.capture_interface_b.trim()
        : "";
      if (params.capture_interface_b_enabled === true) {
        subtitle = `A: ${interfaceA || labels.trdpUnconfigured} · B: ${interfaceB || labels.trdpUnconfigured}`;
      } else {
        subtitle = `${labels.trdpCapture}: ${interfaceA || labels.trdpUnconfigured}`;
      }
    } else {
      const linkA = typeof params.link_a_ip === "string" && params.link_a_ip.trim()
        ? params.link_a_ip.trim()
        : "0.0.0.0";
      const linkB = typeof params.link_b_ip === "string" && params.link_b_ip.trim()
        ? params.link_b_ip.trim()
        : "0.0.0.0";
      subtitle = `A: ${linkA} · B: ${params.link_b_enabled === true ? linkB : labels.trdpDisabled}`;
    }
  } else {
    subtitle = tab.endpoint;
  }

  const localAddr = getNetworkClientLocalAddr(tab, networkState);
  return localAddr ? `${subtitle} · ${localAddr}` : subtitle;
}

/**
 * Pane title keeps the Session name as the primary identity. Runtime child sessions include the
 * saved parent name so identical channel names remain distinguishable outside the Sidebar tree.
 */
export function getPaneDisplayTitle(tab: TabInfo, tabsById: Map<string, TabInfo>): string {
  if (!tab.parentId) return tab.name;
  const parent = tabsById.get(tab.parentId);
  if (!parent?.name) return tab.name;
  return `${parent.name} › ${tab.name}`;
}

export function getPaneDisplayLabel(
  tab: TabInfo,
  tabsById: Map<string, TabInfo>,
  labels: SessionPresentationLabels,
  networkState?: SessionPresentationNetworkState,
): string {
  const title = getPaneDisplayTitle(tab, tabsById);
  const subtitle = getSessionSubtitle(tab, labels, networkState).trim();
  return subtitle ? `${title} · ${subtitle}` : title;
}
