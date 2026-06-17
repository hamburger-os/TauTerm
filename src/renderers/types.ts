/**
 * 渲染器共享类型
 */

export type TabState = "disconnected" | "connecting" | "connected" | "transferring" | "error";

export interface TabInfo {
  id: string;
  pluginId: string;
  name: string;
  state: TabState;
  endpoint?: string;
  params?: Record<string, unknown>;
}
