import { forwardRef, useCallback, useEffect, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import TerminalSurface from "./TerminalSurface";
import type { TerminalSurfaceHandle, TerminalSurfaceSource } from "./TerminalSurface";

/** PTY resize 防抖间隔 (ms)：避免拖拽 resize 时 IPC 风暴 */
const RESIZE_DEBOUNCE_MS = 150;

interface TerminalInstanceProps {
  /** 会话 ID，用于数据路由与 PTY resize */
  sessionId: string;
  runtimeId: string | null;
  /** 当用户在终端输入时回调 */
  onData?: (data: string) => void;
  /** 是否已连接 */
  isConnected?: boolean;
  /** 是否为当前活跃标签页 */
  isActive?: boolean;
  /** 终端当前是否真的显示在 Workspace 中（分屏 Pane 或当前标签） */
  isVisible?: boolean;
  /** 当终端就绪时回调，传入 write 函数供父组件注册数据路由 */
  onTermReady?: (writeFn: (data: Uint8Array | string) => void) => void;
  /** 当终端实例卸载时回调，供父组件清理数据路由 */
  onCleanup?: (sessionId: string) => void;
  /** 触发断开当前会话 */
  onDisconnectSession?: () => void;
  /** 当前 Session 是否提供 Protocol Inspector 右侧栏 */
  allowProtocolInspect?: boolean;
  /**
   * 是否为每一行 LF 追加 CR（xterm `convertEol`）。只对**不是 PTY** 的字节流开启：
   * PTY（Local Shell / SSH）的行结束翻译由远端 termios / ConPTY 自己负责。
   */
  convertEol?: boolean;
}

/**
 * PTY 会话的终端适配器。
 *
 * 呈现、主题、渲染器、搜索、剪贴板安全、右键菜单全部属于共享的
 * `TerminalSurface`；这里只保留 PTY 语义：输入路由、挂载前缓冲的回放和
 * `resize_pty` 通知。
 */
const TerminalInstance = forwardRef<TerminalSurfaceHandle, TerminalInstanceProps>(
  function TerminalInstance({
    sessionId,
    runtimeId,
    onData,
    isConnected = false,
    isActive = true,
    isVisible = true,
    onTermReady,
    onCleanup,
    onDisconnectSession,
    allowProtocolInspect = true,
    convertEol = true,
  }, ref) {
    const resizeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

    // 回调 refs：surface 只构造一次，不能持有过期的会话闭包。
    const onDataRef = useRef(onData);
    onDataRef.current = onData;
    const onTermReadyRef = useRef(onTermReady);
    onTermReadyRef.current = onTermReady;
    const onCleanupRef = useRef(onCleanup);
    onCleanupRef.current = onCleanup;
    const sessionIdRef = useRef(sessionId);
    sessionIdRef.current = sessionId;

    const source = useMemo<TerminalSurfaceSource>(() => ({
      input: (data) => onDataRef.current?.(data),
      attach: (write) => {
        // Registering the writer drains the pre-mount buffer inside TerminalView;
        // the returned cleanup unregisters it so no bytes target a disposed xterm.
        onTermReadyRef.current?.(write);
        return () => onCleanupRef.current?.(sessionIdRef.current);
      },
    }), []);

    const handleFit = useCallback((cols: number, rows: number) => {
      if (!runtimeId) return;
      if (resizeTimerRef.current) clearTimeout(resizeTimerRef.current);
      resizeTimerRef.current = setTimeout(() => {
        invoke("resize_pty", { sessionId, runtimeId, cols, rows }).catch(() => {});
      }, RESIZE_DEBOUNCE_MS);
    }, [runtimeId, sessionId]);

    useEffect(() => () => {
      if (resizeTimerRef.current) {
        clearTimeout(resizeTimerRef.current);
        resizeTimerRef.current = null;
      }
    }, []);

    return (
      <TerminalSurface
        ref={ref}
        sessionId={sessionId}
        source={source}
        inputEnabled={isConnected}
        isVisible={isVisible}
        isActive={isActive}
        convertEol={convertEol}
        allowProtocolInspect={allowProtocolInspect}
        // 仅连接中显示「断开连接」：retained 的只读终端没有可结束的会话。
        onDisconnectSession={isConnected ? onDisconnectSession : undefined}
        onFit={handleFit}
      />
    );
  },
);

export default TerminalInstance;
