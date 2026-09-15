/**
 * TauTerm 内核 — 插件注册表
 *
 * 前端插件注册中心。插件通过 `registerPlugin()` 向内核注册 manifest 与可选贡献点。
 * 公共 UI 只消费稳定 contribution，不解释具体协议的运行态字段。
 */

import type { ComponentType, ReactNode } from "react";
import type { IconName } from "../components/common/Icon";
import i18n from "../i18n";
import type {
  SessionPresentation,
  SessionReconnectContext,
  SessionReconnectGuardResult,
} from "./plugin-contracts";

export type {
  SessionPresentation,
  SessionReconnectContext,
  SessionReconnectGuardResult,
} from "./plugin-contracts";

// ── Types ───────────────────────────────────────────

export type ContentType = "terminal" | "file_browser" | "stats_dashboard" | "custom";

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

export interface SessionConnectOptions {
  transferEnabled: boolean;
  transferProtocol?: string;
  sendBarEnabled: boolean;
}

export interface ConnectFormProps {
  params: Record<string, unknown>;
  onChange: (params: Record<string, unknown>) => void;
  endpoints?: EndpointInfo[];
  endpoint?: string;
  onEndpointChange?: (endpoint: string) => void;
  onRefreshEndpoints?: () => void;
  refreshingEndpoints?: boolean;
  disabled?: boolean;
  sessionOptions?: SessionConnectOptions;
  onSessionOptionsChange?: (options: SessionConnectOptions) => void;
}

export interface EndpointInfo {
  name: string;
  description: string;
  params?: Record<string, unknown>;
}

export interface ToolbarItem {
  id: string;
  icon: IconName;
  label: string;
  position: "left" | "center" | "right";
  onClick: () => void;
}

export interface ContextMenuItem {
  id: string;
  label: string;
  onClick: (tabId: string) => void;
}

export interface BottomPanelDef {
  id: string;
  title: string;
  component: ComponentType<{ sessionId: string }>;
}

export interface StatusBarContext {
  sessionId: string;
  /** 协议无关最小快照；插件私有状态通过 runtimeStore 读取。 */
  activeTab: StatusBarTab | null;
}

export type StatusBarSessionState = "disconnected" | "connecting" | "connected" | "transferring";

export interface StatusBarTab {
  id: string;
  pluginId: string;
  state: StatusBarSessionState;
  endpoint: string;
  params?: Record<string, unknown>;
  connectedAt?: number | null;
  stats?: {
    txBytes: number;
    rxBytes: number;
    rxPackets?: number;
    txPackets?: number;
  };
}

/**
 * 响应式收缩策略。
 * - preserve: 核心身份/应用状态，窄窗口仍保留；
 * - auto: 按 priority 分级收缩；
 * - early: 辅助信息，在第一档窄屏即隐藏。
 */
export type StatusBarOverflow = "preserve" | "auto" | "early";

/**
 * 协议插件状态栏项。
 *
 * 插件只贡献左侧的 Session/协议运行事实；右侧应用级区域由应用壳独占。
 * 每个 item 由 StatusBar 独立 React host 渲染，因此 item 内部可以安全使用 hooks。
 */
export interface StatusBarItem {
  id: string;
  priority: number;
  overflow?: StatusBarOverflow;
  when?: (context: StatusBarContext) => boolean;
  render: (context: StatusBarContext) => ReactNode;
}

export type StatusBarRenderer = (context: StatusBarContext) => ReactNode;
export type LocaleMap = Record<string, Record<string, string>>;

/**
 * 插件私有前端运行态。快照类型故意为 unknown：公共层只能订阅/转交，不能读取协议字段。
 * `revision()` 必须在每次可观察变更后单调递增，用于共享树等聚合消费者安全订阅。
 */
export interface PluginRuntimeStore {
  subscribe: (listener: () => void) => () => void;
  getSnapshot: (sessionId: string) => unknown;
  revision: () => number;
  release?: (sessionId: string) => void;
}

export interface PluginSendContext {
  sessionId: string;
  params: Record<string, unknown>;
  data: string | Uint8Array;
  /** 走协议无关 `send_data` 路径，并保持统一 TX 订阅/统计行为。 */
  sendDefault: (sessionId: string, data: string | Uint8Array) => Promise<void>;
}

export interface PluginSessionTreeMenuItem {
  id: string;
  label: string;
  icon?: IconName;
  danger?: boolean;
  run: () => void | Promise<void>;
}

export interface PluginSessionTreeChild {
  id: string;
  name: string;
  subtitle: string;
  state: StatusBarSessionState;
  selected?: boolean;
  onSelect?: () => void;
  menuItems?: PluginSessionTreeMenuItem[];
}

/** 插件为左侧 Session 树贡献的后台实体；普通 Session child 仍由 Core 自己渲染。 */
export interface PluginSessionTreeContribution {
  groupKey?: (params: Record<string, unknown>, fallback: string) => string;
  children: (
    sessionId: string,
    params: Record<string, unknown>,
    runtimeSnapshot: unknown,
  ) => PluginSessionTreeChild[];
  onParentSelect?: (
    sessionId: string,
    params: Record<string, unknown>,
    runtimeSnapshot: unknown,
  ) => void;
}

export interface PluginRegistration {
  manifest: PluginManifest;
  connectForm?: ComponentType<ConnectFormProps>;
  /** 新建配置时的唯一默认值来源。 */
  defaultConnectionParams?: () => Record<string, unknown>;
  /** 将持久化/编辑态参数收束到插件当前 schema；不承担旧版本兼容迁移。 */
  normalizeConnectionParams?: (params: Record<string, unknown>) => Record<string, unknown>;
  isConnectionConfigValid?: (params: Record<string, unknown>) => boolean;
  /** 将瞬态编辑参数投影为前端内存中的安全 Saved Session 参数。 */
  persistedConnectionParams?: (
    params: Record<string, unknown>,
    sessionId: string,
  ) => Record<string, unknown>;
  reconnectGuard?: (
    context: SessionReconnectContext,
  ) => SessionReconnectGuardResult | Promise<SessionReconnectGuardResult>;
  /** 同步默认展示和动态摘要；需要宿主调用的默认名使用 resolveDefaultSessionName。 */
  sessionPresentation?: SessionPresentation;
  resolveDefaultSessionName?: (
    params: Record<string, unknown>,
    endpoint: string,
  ) => string | Promise<string>;
  /** 插件私有 Session-scoped 运行态。 */
  runtimeStore?: PluginRuntimeStore;
  /** 覆盖公共 send_data 的协议专属目标/扇出策略。 */
  sendData?: (context: PluginSendContext) => Promise<void>;
  /** 左侧 Session 树中的插件私有后台实体。 */
  sessionTree?: PluginSessionTreeContribution;
  /** 全局 SendBar 中的插件专属目标选择区。 */
  sendTarget?: ComponentType<{ sessionId: string }>;
  /** 插件决定目标选择区是否占用 SendBar 的固定附加行。 */
  sendTargetVisible?: (params: Record<string, unknown>) => boolean;
  /** TerminalView 查询插件运行态后决定是否本地回显。 */
  terminalLocalEcho?: (runtimeSnapshot: unknown) => boolean;
  toolbarItems?: ToolbarItem[];
  contextMenuItems?: ContextMenuItem[];
  bottomPanels?: BottomPanelDef[];
  statusBarItems?: StatusBarItem[];
  locales?: LocaleMap;
  customView?: ComponentType<{ sessionId: string }>;
}

class PluginRegistry {
  private plugins = new Map<string, PluginRegistration>();

  register(registration: PluginRegistration): void {
    const id = registration.manifest.id;
    if (this.plugins.has(id)) {
      throw new Error(`[PluginRegistry] 插件 "${id}" 重复注册`);
    }

    const statusIds = new Set<string>();
    for (const item of registration.statusBarItems ?? []) {
      const itemId = item.id.trim();
      if (!itemId) {
        throw new Error(`[PluginRegistry] 插件 "${id}" 存在空状态栏项 ID`);
      }
      if (statusIds.has(itemId)) {
        throw new Error(`[PluginRegistry] 插件 "${id}" 状态栏项 "${itemId}" 重复注册`);
      }
      statusIds.add(itemId);
    }

    this.plugins.set(id, registration);
    for (const [language, resources] of Object.entries(registration.locales ?? {})) {
      i18n.addResourceBundle(language, "translation", { [id]: resources }, true, true);
    }
  }

  unregister(pluginId: string): void {
    this.plugins.delete(pluginId);
  }

  get(pluginId: string): PluginRegistration | undefined {
    return this.plugins.get(pluginId);
  }

  getAll(): PluginRegistration[] {
    return Array.from(this.plugins.values());
  }

  getByCapability(capability: string): PluginRegistration[] {
    return this.getAll().filter((plugin) => plugin.manifest.capabilities.includes(capability));
  }

  supportsSendBar(pluginId: string): boolean {
    return this.get(pluginId)?.manifest.send_bar === true;
  }

  resolveSendBarEnabled(pluginId: string, requested?: boolean): boolean {
    return this.supportsSendBar(pluginId) && requested !== false;
  }

  getToolbarItems(pluginId: string): ToolbarItem[] {
    return this.get(pluginId)?.toolbarItems ?? [];
  }

  getContextMenuItems(pluginId: string): ContextMenuItem[] {
    return this.get(pluginId)?.contextMenuItems ?? [];
  }

  getBottomPanels(pluginId: string): BottomPanelDef[] {
    return this.get(pluginId)?.bottomPanels ?? [];
  }
}

export const pluginRegistry = new PluginRegistry();

export function registerPlugin(registration: PluginRegistration): void {
  pluginRegistry.register(registration);
}
