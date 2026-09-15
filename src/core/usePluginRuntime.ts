import { useSyncExternalStore } from "react";
import { pluginRegistry } from "./plugin-registry";

const EMPTY_RUNTIME = Object.freeze({});

/**
 * 读取某个 Session 的插件私有运行态。
 *
 * Hook 本身只知道 `unknown` 快照；具体插件 renderer/contribution 负责强类型解释。
 * 当插件没有 runtimeStore 时返回稳定空对象，保证 hooks 调用顺序不依赖协议类型。
 */
export function usePluginRuntime<T = unknown>(pluginId: string, sessionId: string): T {
  const store = pluginRegistry.get(pluginId)?.runtimeStore;
  return useSyncExternalStore(
    store?.subscribe ?? (() => () => {}),
    () => (store?.getSnapshot(sessionId) ?? EMPTY_RUNTIME) as T,
    () => (store?.getSnapshot(sessionId) ?? EMPTY_RUNTIME) as T,
  );
}

/**
 * 聚合订阅全部插件 runtime。主要供 SessionSidebar 这类跨 Session 容器使用；
 * 具体协议字段仍通过各插件 `sessionTree` contribution 解释。
 */
export function usePluginRuntimeRevision(): number {
  const stores = pluginRegistry
    .getAll()
    .map(plugin => plugin.runtimeStore)
    .filter((store): store is NonNullable<typeof store> => Boolean(store));

  return useSyncExternalStore(
    (listener) => {
      const unlisten = stores.map(store => store.subscribe(listener));
      return () => unlisten.forEach(dispose => dispose());
    },
    () => stores.reduce((sum, store) => sum + store.revision(), 0),
    () => stores.reduce((sum, store) => sum + store.revision(), 0),
  );
}
