/**
 * TauTerm 内核 — 插件目录与注册表
 *
 * 插件模块只导出静态 definition；应用 composition root 通过 `installPlugins()`
 * 一次性安装内建插件。公共 UI 只消费稳定 contribution，不解释具体协议运行态。
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

export type StatusBarOverflow = "preserve" | "auto" | "early";

export interface StatusBarItem {
  id: string;
  priority: number;
  overflow?: StatusBarOverflow;
  when?: (context: StatusBarContext) => boolean;
  render: (context: StatusBarContext) => ReactNode;
}

export type StatusBarRenderer = (context: StatusBarContext) => ReactNode;
export type LocaleMap = Record<string, Record<string, string>>;

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
  sendDefault: (sessionId: string, data: string | Uint8Array) => Promise<void>;
}

export interface PluginSendTargetVisibilityContext {
  sessionId: string;
  params: Record<string, unknown>;
  runtimeSnapshot: unknown;
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

export interface PluginRightSidebarPanel {
  id: string;
  when?: (params: Record<string, unknown>) => boolean;
  component: ComponentType<{ sessionId: string; isConnected: boolean }>;
}

export interface PluginRightSidebarContribution {
  available?: (params: Record<string, unknown>) => boolean;
  panels?: PluginRightSidebarPanel[];
}

export interface PluginWorkspaceContribution {
  availability: "connected" | "always";
}

export interface PluginRegistration {
  manifest: PluginManifest;
  connectForm?: ComponentType<ConnectFormProps>;
  defaultConnectionParams?: () => Record<string, unknown>;
  defaultSessionOptions?: () => SessionConnectOptions;
  normalizeConnectionParams?: (params: Record<string, unknown>) => Record<string, unknown>;
  prepareConnectionParams?: (
    params: Record<string, unknown>,
  ) => Record<string, unknown> | Promise<Record<string, unknown>>;
  isConnectionConfigValid?: (params: Record<string, unknown>, endpoint?: string) => boolean;
  resolveEndpoint?: (
    params: Record<string, unknown>,
    endpoint: string,
  ) => string | Promise<string>;
  persistedConnectionParams?: (
    params: Record<string, unknown>,
    sessionId: string,
  ) => Record<string, unknown>;
  reconnectGuard?: (
    context: SessionReconnectContext,
  ) => SessionReconnectGuardResult | Promise<SessionReconnectGuardResult>;
  formatSessionError?: (error: unknown, operation: "connect" | "open_channel") => string;
  canCreateElevatedSession?: (params: Record<string, unknown>) => boolean;
  sessionPresentation?: SessionPresentation;
  resolveDefaultSessionName?: (
    params: Record<string, unknown>,
    endpoint: string,
  ) => string | Promise<string>;
  runtimeStore?: PluginRuntimeStore;
  sendData?: (context: PluginSendContext) => Promise<void>;
  sessionTree?: PluginSessionTreeContribution;
  sendTarget?: ComponentType<{ sessionId: string; disabled?: boolean }>;
  sendTargetVisible?: (context: PluginSendTargetVisibilityContext) => boolean;
  terminalLocalEcho?: (runtimeSnapshot: unknown) => boolean;
  appOverlay?: ComponentType;
  rightSidebar?: PluginRightSidebarContribution;
  workspace?: PluginWorkspaceContribution;
  toolbarItems?: ToolbarItem[];
  contextMenuItems?: ContextMenuItem[];
  bottomPanels?: BottomPanelDef[];
  statusBarItems?: StatusBarItem[];
  locales?: LocaleMap;
  customView?: ComponentType<{ sessionId: string }>;
}

/** 编译期内建插件定义。定义阶段不得修改全局注册表。 */
export type PluginDefinition = Readonly<PluginRegistration>;

export function definePlugin(registration: PluginRegistration): PluginDefinition {
  return registration;
}

function validatePlugin(registration: PluginDefinition): void {
  const id = registration.manifest.id;
  if (!id || id !== id.trim() || id !== id.toLowerCase()) {
    throw new Error(`[PluginRegistry] 插件 ID 必须是非空、trim 后的小写值: "${id}"`);
  }
  if (registration.manifest.capabilities.includes("connection")) {
    if (!registration.connectForm) {
      throw new Error(`[PluginRegistry] 连接插件 "${id}" 必须注册 connectForm`);
    }
    if (!registration.sessionPresentation?.defaultName) {
      throw new Error(`[PluginRegistry] 连接插件 "${id}" 必须注册 sessionPresentation.defaultName`);
    }
  }

  const statusIds = new Set<string>();
  for (const item of registration.statusBarItems ?? []) {
    const itemId = item.id.trim();
    if (!itemId) {
      throw new Error(`[PluginRegistry] 插件 "${id}" 存在空状态栏项 ID`);
    }
    if (statusIds.has(itemId)) {
      throw new Error(`[PluginRegistry] 插件 "${id}" 状态栏项 "${itemId}" 重复`);
    }
    statusIds.add(itemId);
  }
}

class PluginRegistry {
  private readonly plugins = new Map<string, PluginDefinition>();

  install(registrations: readonly PluginDefinition[]): void {
    const batchIds = new Set<string>();
    for (const registration of registrations) {
      validatePlugin(registration);
      const id = registration.manifest.id;
      if (batchIds.has(id) || this.plugins.has(id)) {
        throw new Error(`[PluginRegistry] 插件 "${id}" 重复注册`);
      }
      batchIds.add(id);
    }

    for (const registration of registrations) {
      const id = registration.manifest.id;
      this.plugins.set(id, registration);
      for (const [language, resources] of Object.entries(registration.locales ?? {})) {
        i18n.addResourceBundle(language, "translation", { [id]: resources }, true, true);
      }
    }
  }

  get(pluginId: string): PluginDefinition | undefined {
    return this.plugins.get(pluginId);
  }

  getAll(): PluginDefinition[] {
    return Array.from(this.plugins.values());
  }

  getByCapability(capability: string): PluginDefinition[] {
    return this.getAll().filter((plugin) => plugin.manifest.capabilities.includes(capability));
  }

  getSessionDefaultName(
    pluginId: string,
    params: Record<string, unknown>,
    endpoint: string,
  ): string {
    const plugin = this.get(pluginId);
    const normalizedEndpoint = endpoint.trim();
    if (!plugin) {
      return normalizedEndpoint
        ? `${pluginId.toUpperCase()} @ ${normalizedEndpoint}`
        : pluginId.toUpperCase();
    }

    const normalizedParams = plugin.normalizeConnectionParams?.(params) ?? params;
    const presentationName = plugin.sessionPresentation
      ?.defaultName?.(normalizedParams, endpoint)
      ?.trim() ?? "";
    const fallbackName = normalizedEndpoint
      ? `${plugin.manifest.name} @ ${normalizedEndpoint}`
      : plugin.manifest.name;
    return presentationName || fallbackName;
  }

  async resolveSessionDefaultName(
    pluginId: string,
    params: Record<string, unknown>,
    endpoint: string,
  ): Promise<string> {
    const plugin = this.get(pluginId);
    const normalizedParams = plugin?.normalizeConnectionParams?.(params) ?? params;
    const resolvedName = plugin?.resolveDefaultSessionName
      ? (await plugin.resolveDefaultSessionName(normalizedParams, endpoint)).trim()
      : "";
    return resolvedName || this.getSessionDefaultName(pluginId, normalizedParams, endpoint);
  }

  getDefaultSessionOptions(pluginId: string): SessionConnectOptions {
    const plugin = this.get(pluginId);
    return plugin?.defaultSessionOptions?.() ?? {
      transferEnabled: false,
      sendBarEnabled: plugin?.manifest.send_bar === true,
    };
  }

  resolveSessionOptions(
    pluginId: string,
    requested: Partial<SessionConnectOptions> = {},
  ): SessionConnectOptions {
    const defaults = this.getDefaultSessionOptions(pluginId);
    return {
      transferEnabled: requested.transferEnabled ?? defaults.transferEnabled,
      transferProtocol: requested.transferProtocol ?? defaults.transferProtocol,
      sendBarEnabled: this.resolveSendBarEnabled(
        pluginId,
        requested.sendBarEnabled ?? defaults.sendBarEnabled,
      ),
    };
  }

  supportsSendBar(pluginId: string): boolean {
    return this.get(pluginId)?.manifest.send_bar === true;
  }

  resolveSendBarEnabled(pluginId: string, requested?: boolean): boolean {
    return this.supportsSendBar(pluginId) && requested !== false;
  }

  resolveSendTargetVisible(
    pluginId: string,
    sessionId: string,
    params: Record<string, unknown>,
  ): boolean {
    const plugin = this.get(pluginId);
    if (!plugin?.sendTarget) return false;
    return plugin.sendTargetVisible?.({
      sessionId,
      params,
      runtimeSnapshot: plugin.runtimeStore?.getSnapshot(sessionId),
    }) ?? true;
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

export function installPlugins(registrations: readonly PluginDefinition[]): void {
  pluginRegistry.install(registrations);
}
