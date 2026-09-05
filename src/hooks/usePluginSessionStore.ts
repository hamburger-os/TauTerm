/**
 * 插件会话共享 store hook（TFTP / iperf 等"模块级持久事件系统"的统一实现）
 *
 * 背景：TftpSessionView 与 IperfSessionView 各自维护一份 ~150 行的模块级
 * store/subscribers/inited/watchdogs 样板，且存在共同缺陷（逐会话注册的
 * Tauri 监听器永不注销、断连重连后重复注册、getSnapshot 在渲染期突变）。
 *
 * 本 hook 的约定：
 * - 监听器按会话注册（`init` 返回 UnlistenFn[]），生命周期由
 *   `session-disconnected` 终结：`keepAlive=false` 的会话断连时注销并清除，
 *   重连时重注册（修复泄漏）；`keepAlive=true` 的会话常驻进程生命周期
 *   （TFTP 后台传输依赖：组件卸载后传输继续、done 事件不丢）。
 * - `session-disconnected` / `session-connected` 全局各只监听一次，按会话
 *   分发（修复"每会话挂一份全局监听器，N 个会话每个全局事件空转 N 次"）；
 *   后者在重连后刷新 getStatus（注册与重连间隙的查询拿到的是断连默认值）。
 * - getSnapshot 纯读（缺失返回稳定空快照），渲染期不再突变 store。
 * - 看门狗注册表触发时自删条目，key 加会话前缀防跨会话冲突。
 * - 组件卸载只退订 React 订阅，不注销 Tauri 监听器（tab 切换不丢事件）。
 */
import { useCallback, useEffect, useRef, useSyncExternalStore } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ═══════════════════════════════════════════════════════════════════
// 类型
// ═══════════════════════════════════════════════════════════════════

/** 会话级 API：init 回调里用它读写状态与注册看门狗（对 sessionId 稳定） */
export interface SessionStoreApi<TState> {
  readonly sessionId: string;
  getState(): TState;
  /** 不可变补丁写入 + 通知订阅者 */
  setState(patch: Partial<TState> | ((prev: TState) => Partial<TState>)): void;
  /** 注册看门狗：同名 key 先清旧定时器；触发时自删条目并执行 fn */
  setWatchdog(key: string, fn: () => void, timeoutMs: number): void;
  clearWatchdog(key: string): void;
}

export interface PluginSessionStoreOptions<TState> {
  /** 初始状态（store 不存在时调用一次；断连重注册保留已有状态） */
  createState: () => TState;
  /**
   * 注册本会话的事件监听；返回的清理函数在 keepAlive=false 的会话断开时执行。
   * 只在会话首次注册时调用一次。
   */
  init: (api: SessionStoreApi<TState>) => UnlistenFn[] | Promise<UnlistenFn[]>;
  /**
   * true = 会话断开时保留监听器与 store（后台传输常驻，TFTP）。
   * 缺省 false = 断开时执行 init 返回的清理并注销会话（重连时重注册）。
   */
  keepAlive?: boolean;
  /** 会话永久删除时释放插件自有的模块级资源/注册表。 */
  onRelease?: () => void;
  /** 会话断开时的状态处理（返回补丁）；缺省：keepAlive ? 原样保留 : 清空重建 */
  onSessionDisconnected?: (state: TState) => Partial<TState> | undefined;
  /**
   * 后端状态加载（invoke("xxx_get_status")），返回补丁。
   * 注册时执行一次；每次 session-connected（重连）时刷新——注册与重连
   * 间隙中的查询会拿到"未连接"默认值，重连后的真实参数靠刷新送达。
   */
  getStatus?: (
    sessionId: string,
    api: SessionStoreApi<TState>
  ) => Promise<Partial<TState> | undefined>;
}

// ═══════════════════════════════════════════════════════════════════
// 模块级注册表（进程生命周期内唯一一份）
// ═══════════════════════════════════════════════════════════════════

const stores = new Map<string, unknown>();
const subscribers = new Map<string, Set<() => void>>();
/** init 返回的清理函数（keepAlive=false 的会话断开时执行） */
const cleanupFns = new Map<string, () => void>();
/** 永久删除时执行；与 sessionOpts 分离，确保 keepAlive=false 会话断开后仍可释放插件资源。 */
const releaseFns = new Map<string, () => void>();
const apiCache = new Map<string, SessionStoreApi<unknown>>();
const sessionOpts = new Map<string, PluginSessionStoreOptions<unknown>>();
/** 注册代际：断连后重注册递增，用于废弃旧代在途的 init/getStatus 结果 */
const generations = new Map<string, number>();
/** 看门狗定时器（key 带 sessionId 前缀） */
const watchdogs = new Map<string, ReturnType<typeof setTimeout>>();
let globalDisconnectReady: Promise<void> | null = null;
let globalConnectReady: Promise<void> | null = null;

/** 稳定空快照：getSnapshot 在 store 缺失时返回它（不触发渲染期写入） */
const EMPTY_SNAPSHOT: unknown = Object.freeze({});

// ═══════════════════════════════════════════════════════════════════
// 内部实现
// ═══════════════════════════════════════════════════════════════════

/** 清除会话的全部看门狗（断连时调用，防陈旧定时器跨代触发新测试状态） */
function clearSessionWatchdogs(sid: string): void {
  const prefix = `${sid}:`;
  for (const key of [...watchdogs.keys()]) {
    if (!key.startsWith(prefix)) continue;
    const timer = watchdogs.get(key);
    if (timer) clearTimeout(timer);
    watchdogs.delete(key);
  }
}

/** session-disconnected 全局只监听一次，按会话分发 */
function ensureGlobalDisconnectListener(): Promise<void> {
  if (!globalDisconnectReady) {
    globalDisconnectReady = (async () => {
      await listen<{ session_id: string }>("session-disconnected", (event) => {
        const sid = event.payload.session_id;
        const opts = sessionOpts.get(sid);
        if (!opts) return;
        const api = apiCache.get(sid);
        if (api) {
          const patch = opts.onSessionDisconnected?.(
            api.getState() as never
          );
          if (patch) api.setState(patch as never);
        }
        if (!opts.keepAlive) {
          // 注销监听器、清除看门狗并注销会话注册。store 保留：组件可能
          // 仍挂载于断开态的 tab；重注册时 ensureSession 不复建已有 store，
          // 记录/参数经 onSessionDisconnected 补丁处理后跨断连保留
          cleanupFns.get(sid)?.();
          cleanupFns.delete(sid);
          sessionOpts.delete(sid);
          clearSessionWatchdogs(sid);
        }
      });
    })().catch((e) => {
      // 注册失败（IPC 抖动）：重置为 null 使下次挂载自动重试，
      // 否则所有会话的断连清理会全应用生命周期静默失效
      console.error(
        "[usePluginSessionStore] session-disconnected 注册失败，下次挂载将重试:",
        e
      );
      globalDisconnectReady = null;
    });
  }
  return globalDisconnectReady;
}

/**
 * session-connected 全局只监听一次，按会话分发：重连后重新查询后端状态。
 *
 * getStatus 只在注册时执行一次；重连（先断连后重连）期间的那次注册查询
 * 拿到的是"未连接"默认值，重连后的真实参数（如版本切换后的端口联动）
 * 必须靠此刷新送达前端。resolve 时按代际守卫，防过期查询覆盖新状态。
 */
function ensureGlobalConnectListener(): Promise<void> {
  if (!globalConnectReady) {
    globalConnectReady = (async () => {
      await listen<{ session_id: string }>("session-connected", (event) => {
        const sid = event.payload.session_id;
        const opts = sessionOpts.get(sid);
        if (!opts?.getStatus) return;
        const api = apiCache.get(sid);
        if (!api) return;
        const gen = generations.get(sid);
        opts
          .getStatus(sid, api)
          .then((patch) => {
            if (patch && generations.get(sid) === gen) api.setState(patch);
          })
          .catch(() => {});
      });
    })().catch((e) => {
      // 注册失败（IPC 抖动）：重置为 null 使下次挂载自动重试
      console.error(
        "[usePluginSessionStore] session-connected 注册失败，下次挂载将重试:",
        e
      );
      globalConnectReady = null;
    });
  }
  return globalConnectReady;
}

function makeApi<TState>(sessionId: string): SessionStoreApi<TState> {
  return {
    sessionId,
    getState: () => (stores.get(sessionId) as TState | undefined) ?? (EMPTY_SNAPSHOT as TState),
    setState: (patch) => {
      const prev = stores.get(sessionId) as TState | undefined;
      if (!prev) return;
      const p = typeof patch === "function" ? patch(prev) : patch;
      stores.set(sessionId, { ...prev, ...p });
      subscribers.get(sessionId)?.forEach((cb) => cb());
    },
    setWatchdog: (key, fn, timeoutMs) => {
      const fullKey = `${sessionId}:${key}`;
      const old = watchdogs.get(fullKey);
      if (old) clearTimeout(old);
      watchdogs.set(
        fullKey,
        setTimeout(() => {
          watchdogs.delete(fullKey); // 触发即自删，不留残留条目
          fn();
        }, timeoutMs)
      );
    },
    clearWatchdog: (key) => {
      const fullKey = `${sessionId}:${key}`;
      const old = watchdogs.get(fullKey);
      if (old) {
        clearTimeout(old);
        watchdogs.delete(fullKey);
      }
    },
  };
}

/** 幂等注册（渲染期调用安全；StrictMode 双挂载由同步守卫拦截） */
function ensureSession<TState>(
  sessionId: string,
  options: PluginSessionStoreOptions<TState>
): void {
  if (sessionOpts.has(sessionId)) return;
  // 注册代际：断连后重注册时递增，旧代在途的 init/getStatus 结果据此废弃
  const gen = (generations.get(sessionId) ?? 0) + 1;
  generations.set(sessionId, gen);
  sessionOpts.set(sessionId, options as PluginSessionStoreOptions<unknown>);
  if (options.onRelease) {
    releaseFns.set(sessionId, options.onRelease);
  }
  // 已有 store 不复建：断连补丁（记录标 failed、保留参数/历史）跨断连保留
  if (!stores.has(sessionId)) {
    stores.set(sessionId, options.createState());
  }
  const api = makeApi<TState>(sessionId);
  apiCache.set(sessionId, api as SessionStoreApi<unknown>);
  Promise.resolve(options.init(api))
    .then((unlisten) => {
      // 异步注册完成前会话已断开或被新一代注册取代 → 立即撤销
      if (!sessionOpts.has(sessionId) || generations.get(sessionId) !== gen) {
        unlisten.forEach((f) => f());
        return;
      }
      cleanupFns.set(sessionId, () => unlisten.forEach((f) => f()));
    })
    .catch((e) => console.error(`[usePluginSessionStore] 监听器注册失败 (${sessionId}):`, e));
  options
    .getStatus?.(sessionId, api)
    .then((patch) => {
      if (patch && sessionOpts.has(sessionId) && generations.get(sessionId) === gen) {
        api.setState(patch);
      }
    })
    .catch(() => {});
}

/**
 * 释放会话的全部 hook 资源（监听器、store、看门狗、订阅）。
 *
 * 供会话永久删除路径调用（SessionContext.deleteSession）：keepAlive 会话
 * （TFTP）的监听器/状态按设计常驻进程（后台传输依赖），但会话删除后
 * 不再有存续意义——不清理则每个创建过的会话永久泄漏监听器与 Map 条目。
 */
export function releaseSessionStore(sessionId: string): void {
  cleanupFns.get(sessionId)?.();
  cleanupFns.delete(sessionId);
  try {
    releaseFns.get(sessionId)?.();
  } catch (error) {
    console.warn(`[usePluginSessionStore] 释放插件资源失败 (${sessionId}):`, error);
  }
  releaseFns.delete(sessionId);
  sessionOpts.delete(sessionId);
  clearSessionWatchdogs(sessionId);
  stores.delete(sessionId);
  apiCache.delete(sessionId);
  subscribers.delete(sessionId);
  generations.delete(sessionId);
}

function subscribe(sessionId: string, cb: () => void): () => void {
  if (!subscribers.has(sessionId)) subscribers.set(sessionId, new Set());
  subscribers.get(sessionId)!.add(cb);
  return () => {
    subscribers.get(sessionId)?.delete(cb);
  };
}

// ═══════════════════════════════════════════════════════════════════
// Hook
// ═══════════════════════════════════════════════════════════════════

/**
 * 插件会话 store：返回当前状态与稳定 api（init 回调持有）。
 *
 * 同一 sessionId 只注册一次（幂等）；会话断开后（keepAlive=false）重挂载
 * 会重新注册监听器。组件卸载仅退订 React 订阅。
 */
export function usePluginSessionStore<TState>(
  sessionId: string,
  options: PluginSessionStoreOptions<TState>
): { state: TState; api: SessionStoreApi<TState> } {
  const optionsRef = useRef(options);
  optionsRef.current = options;

  // 渲染期幂等注册（会话级状态在首次渲染即可用）
  ensureSession(sessionId, options);

  useEffect(() => {
    void ensureGlobalDisconnectListener();
    void ensureGlobalConnectListener();
  }, [sessionId]);

  const state = useSyncExternalStore(
    useCallback((cb: () => void) => subscribe(sessionId, cb), [sessionId]),
    useCallback(
      () => (stores.get(sessionId) as TState | undefined) ?? (EMPTY_SNAPSHOT as TState),
      [sessionId]
    )
  );

  return { state, api: apiCache.get(sessionId) as SessionStoreApi<TState> };
}
