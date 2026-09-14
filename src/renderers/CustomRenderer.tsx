/**
 * 自定义渲染器
 *
 * 委托给插件自己的 React 组件。适用于 `content_type: "custom"` 的插件。
 * 插件在 registerPlugin() 时提供 customView 组件。
 */

import type { FC } from "react";
import { pluginRegistry } from "../core/plugin-registry";
import type { TabInfo } from "./types";

interface CustomRendererProps {
  tab: TabInfo;
}

const CustomRenderer: FC<CustomRendererProps> = ({ tab }) => {
  const plugin = pluginRegistry.get(tab.pluginId);

  if (!plugin?.customView) {
    return (
      <div style={styles.fallback}>
        <p>插件 "{tab.pluginId}" 未提供自定义视图组件。</p>
      </div>
    );
  }

  const CustomView = plugin.customView;
  // Pane identity is intentionally independent from Session identity. A Pane can
  // be reassigned from one custom Session to another, so the view itself must be
  // keyed by the Session or React would retain component-local state across
  // unrelated Sessions that happen to use the same plugin component type.
  return <CustomView key={`${tab.pluginId}:${tab.id}`} sessionId={tab.id} />;
};

const styles: Record<string, React.CSSProperties> = {
  fallback: {
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    height: "100%",
    color: "var(--text-muted)",
  },
};

export default CustomRenderer;
