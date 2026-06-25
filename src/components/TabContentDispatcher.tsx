/**
 * TabContentDispatcher — 统一标签页内容适配器
 *
 * 根据活跃标签页对应插件的 `content_type` 选择渲染器。
 * 支持 terminal、file_browser、stats_dashboard、custom 四种内容类型。
 */

import { useSession } from "../context/SessionContext";
import { pluginRegistry } from "../core/plugin-registry";
import type { ContentType } from "../core/plugin-registry";
import Icon from "./common/Icon";
import TerminalRenderer from "../renderers/TerminalRenderer";
import FileBrowserRenderer from "../renderers/FileBrowserRenderer";
import StatsDashboardRenderer from "../renderers/StatsDashboardRenderer";
import CustomRenderer from "../renderers/CustomRenderer";

/** 空状态占位（无活跃标签页） */
function EmptyState() {
  return (
    <div style={{
      display: "flex",
      flexDirection: "column",
      alignItems: "center",
      justifyContent: "center",
      height: "100%",
      gap: 12,
      color: "var(--text-muted)",
    }}>
      <Icon name="logo" size="2xl" />
      <span style={{ fontSize: 14 }}>
        请新建会话或从侧栏选择一个已保存的会话
      </span>
    </div>
  );
}

/** 未知内容类型占位 */
function UnknownContentType({ type }: { type: string }) {
  return (
    <div style={{
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      height: "100%",
      color: "var(--text-muted)",
    }}>
      未知的内容类型: {type}
    </div>
  );
}

export default function TabContentDispatcher() {
  const { state } = useSession();
  const activeTab = state.tabs.find(t => t.id === state.activeTabId);

  if (!activeTab) {
    return <EmptyState />;
  }

  const plugin = pluginRegistry.get(activeTab.pluginId);
  const contentType: ContentType = plugin?.manifest.content_type ?? "terminal";

  switch (contentType) {
    case "terminal":
      return <TerminalRenderer />;
    case "file_browser":
      return <FileBrowserRenderer tab={activeTab} />;
    case "stats_dashboard":
      return <StatsDashboardRenderer tab={activeTab} />;
    case "custom":
      return <CustomRenderer tab={activeTab} />;
    default:
      return <UnknownContentType type={contentType} />;
  }
}
