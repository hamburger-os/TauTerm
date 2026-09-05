/**
 * TauTerm 内核 — 插件注册表
 *
 * 前端插件注册中心。插件通过 `registerPlugin()` 向内核注册其
 * manifest、UI 组件、翻译资源等。
 */

import type { ComponentType, ReactNode } from "react";
import type { IconName } from "../components/common/Icon";

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
  icon: IconName;
  content_type: ContentType;
  /** 是否提供 TauTerm 全局 SendBar；协议自带发送/发布工作流时应为 false。 */
  send_bar: boolean;
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
  /** 插件发现端点时附带的配置预设；内核只透传。 */
  params?: Record<string, unknown>;
}

/** 工具栏项 */
export interface ToolbarItem {
  id: string;
  icon: IconName;
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

/** 状态栏渲染上下文 */
export interface StatusBarContext {
  /** 活跃会话 ID（无活跃会话时为空串） */
  sessionId: string;
  /** 活跃会话（无活跃会话时为 null） */
  activeTab: StatusBarTab | null;
}

/**
 * 状态栏可见性/渲染所需的最小会话信息（结构化子集）。
 * 用独立结构而非直接引用 SessionContext 的 TabInfo，避免 plugin-registry
 * 与 SessionContext 形成循环依赖（SessionContext 反向 import 了本模块）。
 */
export interface StatusBarTab {
  id: string;
  pluginId: string;
  state: string;
  endpoint: string;
  params?: Record<string, unknown>;
}

/** 状态栏项（声明式描述符） */
export interface StatusBarItem {
  id: string;
  /** 对齐：左 / 右 */
  align: "left" | "right";
  /** 排序优先级：左对齐时数值越大越靠左，右对齐时数值越大越靠右 */
  priority: number;
  /** 可见性谓词：返回 false 则不渲染 */
  when?: (context: StatusBarContext) => boolean;
  /** 渲染函数：返回一个 React 元素（可在内部使用 hooks） */
  render: (context: StatusBarContext) => ReactNode;
}

/** 状态栏项渲染函数 */
export type StatusBarRenderer = (context: StatusBarContext) => ReactNode;

/** 翻译资源映射 */
export type LocaleMap = Record<string, Record<string, string>>;

/** 插件注册对象 */
export interface PluginRegistration {
  manifest: PluginManifest;
  connectForm?: ComponentType<ConnectFormProps>;
  toolbarItems?: ToolbarItem[];
  contextMenuItems?: ContextMenuItem[];
  bottomPanels?: BottomPanelDef[];
  statusBarItems?: StatusBarItem[];
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

  /** 插件是否拥有 TauTerm 全局 SendBar。 */
  supportsSendBar(pluginId: string): boolean {
    return this.get(pluginId)?.manifest.send_bar === true;
  }

  /** 插件能力是上限，会话配置只能关闭支持的 SendBar，不能为不支持的插件强行开启。 */
  resolveSendBarEnabled(pluginId: string, requested?: boolean): boolean {
    return this.supportsSendBar(pluginId) && requested !== false;
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
