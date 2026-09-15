import type { TabInfo } from "../../context/SessionContext";
import { pluginRegistry } from "../../core/plugin-registry";

/**
 * Transitional call-site parameter aliases.
 *
 * Presentation is now fully plugin-owned; common callers may still pass their already-computed
 * auxiliary objects while those dead dependencies are removed from Layout components. They are
 * intentionally opaque here so no protocol concept leaks back into this common module.
 */
export type SessionPresentationLabels = unknown;
export type SessionPresentationNetworkState = unknown;

/**
 * Canonical second-line identity used by Session cards and Pane headers.
 * Root sessions delegate all protocol-specific formatting to the plugin registration. Runtime
 * child sessions keep their concrete runtime endpoint because they are generic child channels.
 */
export function getSessionSubtitle(
  tab: TabInfo,
  _legacyLabels?: SessionPresentationLabels,
  _legacyRuntimeState?: SessionPresentationNetworkState,
): string {
  if (tab.parentId) return tab.endpoint;

  const params = (tab.params ?? {}) as Record<string, unknown>;
  return pluginRegistry
    .get(tab.pluginId)
    ?.sessionPresentation
    ?.subtitle?.(params, tab.endpoint)
    ?.trim()
    || tab.endpoint;
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
  legacyLabels?: SessionPresentationLabels,
  legacyRuntimeState?: SessionPresentationNetworkState,
): string {
  const title = getPaneDisplayTitle(tab, tabsById);
  const subtitle = getSessionSubtitle(tab, legacyLabels, legacyRuntimeState).trim();
  return subtitle ? `${title} · ${subtitle}` : title;
}
