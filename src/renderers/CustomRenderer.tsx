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
  return <CustomView sessionId={tab.id} />;
};

const styles: Record<string, React.CSSProperties> = {
  fallback: {
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    height: "100%",
    color: "var(--text-muted, #888)",
  },
};

export default CustomRenderer;
