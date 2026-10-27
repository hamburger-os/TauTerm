/**
 * 渲染器共享类型
 */

export type TabState = import("../core/session-lifecycle").ConnectionStatus;

export interface TabInfo {
  id: string;
  pluginId: string;
  name: string;
  state: TabState;
  endpoint?: string;
  params?: Record<string, unknown>;
}
