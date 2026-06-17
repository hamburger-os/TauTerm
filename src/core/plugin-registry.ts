/**
 * TauTerm 内核 — 插件注册表
 *
 * 前端插件注册中心。插件通过 `registerPlugin()` 向内核注册其
 * manifest、UI 组件、翻译资源等。
 */

import type { ComponentType, ReactNode } from "react";

// ── Types ───────────────────────────────────────────

/** 内容类型 */
export type ContentType = "terminal" | "file_browser" | "stats_dashboard" | "custom";

/** 插件清单 */
export interface PluginManifest {
  id: string;
  name: string;
  version: string;
  category: string;
  description: string;
  icon: string;
  content_type: ContentType;
  capabilities: string[];
  transfer_protocols: string[];
}

/** 连接表单组件 Props */
export interface ConnectFormProps {
  params: Record<string, unknown>;
  onChange: (params: Record<string, unknown>) => void;
  endpoints?: EndpointInfo[];
}

/** 端点信息 */
export interface EndpointInfo {
  name: string;
  description: string;
}

/** 工具栏项 */
export interface ToolbarItem {
  id: string;
  icon: string;
  label: string;
  position: "left" | "center" | "right";
  onClick: () => void;
}

/** 右键菜单项 */
export interface ContextMenuItem {
  id: string;
  label: string;
  onClick: (tabId: string) => void;
}

/** 底部面板标签页定义 */
export interface BottomPanelDef {
  id: string;
  title: string;
  component: ComponentType<{ sessionId: string }>;
}

/** 状态栏项渲染函数 */
export type StatusBarRenderer = (context: { sessionId: string }) => ReactNode;

/** 翻译资源映射 */
export type LocaleMap = Record<string, Record<string, string>>;

/** 插件注册对象 */
export interface PluginRegistration {
  manifest: PluginManifest;
  connectForm?: ComponentType<ConnectFormProps>;
  toolbarItems?: ToolbarItem[];
  contextMenuItems?: ContextMenuItem[];
  bottomPanels?: BottomPanelDef[];
  statusBarItems?: Array<{ id: string; render: StatusBarRenderer }>;
  locales?: LocaleMap;
  /** 自定义内容视图组件（content_type === "custom" 时使用） */
  customView?: ComponentType<{ sessionId: string }>;
}

// ── Registry ────────────────────────────────────────

class PluginRegistry {
  private plugins = new Map<string, PluginRegistration>();

  /** 注册插件 */
  register(registration: PluginRegistration): void {
    const id = registration.manifest.id;
    if (this.plugins.has(id)) {
      console.warn(`[PluginRegistry] 插件 "${id}" 已注册，将被覆盖`);
    }
    this.plugins.set(id, registration);
  }

  /** 注销插件 */
  unregister(pluginId: string): void {
    this.plugins.delete(pluginId);
  }

  /** 获取插件 */
  get(pluginId: string): PluginRegistration | undefined {
    return this.plugins.get(pluginId);
  }

  /** 获取所有已注册插件 */
  getAll(): PluginRegistration[] {
    return Array.from(this.plugins.values());
  }

  /** 获取具有特定能力的插件列表（用于 ConnectDialog） */
  getByCapability(capability: string): PluginRegistration[] {
    return this.getAll().filter(
      (p) => p.manifest.capabilities.includes(capability)
    );
  }

  /** 获取活跃插件的工具栏项 */
  getToolbarItems(pluginId: string): ToolbarItem[] {
    return this.get(pluginId)?.toolbarItems ?? [];
  }

  /** 获取活跃插件的右键菜单项 */
  getContextMenuItems(pluginId: string): ContextMenuItem[] {
    return this.get(pluginId)?.contextMenuItems ?? [];
  }

  /** 获取活跃插件的底部面板 */
  getBottomPanels(pluginId: string): BottomPanelDef[] {
    return this.get(pluginId)?.bottomPanels ?? [];
  }
}

/** 全局单例 */
export const pluginRegistry = new PluginRegistry();

/** 注册插件（便捷函数） */
export function registerPlugin(registration: PluginRegistration): void {
  pluginRegistry.register(registration);
}
