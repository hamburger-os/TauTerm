import type { TabInfo } from "../../context/SessionContext";
import { resolveSessionSubtitle } from "../../core/plugin-contracts";
import { pluginRegistry } from "../../core/plugin-registry";

/** Canonical second-line identity used by Session cards and Pane headers. */
export function getSessionSubtitle(tab: TabInfo): string {
  return resolveSessionSubtitle(
    pluginRegistry.get(tab.pluginId)?.sessionPresentation,
    {
      endpoint: tab.endpoint,
      params: tab.params,
      parentId: tab.parentId,
    },
  );
}

export function getPaneDisplayTitle(tab: TabInfo, tabsById: Map<string, TabInfo>): string {
  if (!tab.parentId) return tab.name;
  const parent = tabsById.get(tab.parentId);
  if (!parent?.name) return tab.name;
  return `${parent.name} › ${tab.name}`;
}

export function getPaneDisplayLabel(tab: TabInfo, tabsById: Map<string, TabInfo>): string {
  const title = getPaneDisplayTitle(tab, tabsById);
  const subtitle = getSessionSubtitle(tab).trim();
  return subtitle ? `${title} · ${subtitle}` : title;
}
