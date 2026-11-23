import { forwardRef, useCallback, useEffect, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { reportFrontendError } from "../../utils/runtimeDiagnostics";
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
   * 必填：它决定字节流语义，没有安全默认值。
   */
  convertEol: boolean;
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
    convertEol,
  }, ref) {
    const resizeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    /**
     * 最近一次 fit 量到的尺寸。运行时的 `runtimeId` 在终端挂载时可能还没就绪，
     * 那时必须记住尺寸并在就绪后立刻补发——PTY 尺寸与视口不一致会让全屏 TUI
     * 按错误行列排版，而不是简单"看起来小一点"。
     */
    const pendingSizeRef = useRef<{ cols: number; rows: number } | null>(null);
    /**
     * 是否已经发出过第一个尺寸。它决定"立即发送"还是"防抖发送"，因此必须与
     * `sentSizeRef` 分开：一次发送失败可以重试，但不能让后续每一次 fit 都退回
     * 到"首次立即发送"，否则拖拽分屏时又变成每个 fit 一次 IPC。
     */
    const hasSentFirstSizeRef = useRef(false);
    /** 已成功发送过的 `<runtime>:<cols>x<rows>`，避免重复 IPC。 */
    const sentSizeRef = useRef<string | null>(null);
    const runtimeIdRef = useRef(runtimeId);
    runtimeIdRef.current = runtimeId;

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

    const flushSize = useCallback((cols: number, rows: number, identity: string) => {
      const key = `${identity}:${cols}x${rows}`;
      if (sentSizeRef.current === key) return;
      sentSizeRef.current = key;
      hasSentFirstSizeRef.current = true;
      invoke("resize_pty", { sessionId, runtimeId: identity, cols, rows }).catch((error) => {
        // 失败不再静默吞掉：尺寸不一致是"全屏应用整体错位"的直接原因。
        // 清掉去重键让下一次（已防抖的）fit 重试；首个尺寸的判定不回退。
        sentSizeRef.current = null;
        reportFrontendError("terminal.resize", error, `${sessionId} ${cols}x${rows}`);
      });
    }, [sessionId]);

    const handleFit = useCallback((cols: number, rows: number) => {
      pendingSizeRef.current = { cols, rows };
      const identity = runtimeIdRef.current;
      if (!identity) return;
      if (resizeTimerRef.current) {
        clearTimeout(resizeTimerRef.current);
        resizeTimerRef.current = null;
      }
      if (!hasSentFirstSizeRef.current) {
        // 首个尺寸决定 PTY 与视口是否一致：立即发送，不吃防抖延迟。
        flushSize(cols, rows, identity);
        return;
      }
      resizeTimerRef.current = setTimeout(() => {
        resizeTimerRef.current = null;
        const size = pendingSizeRef.current;
        const currentIdentity = runtimeIdRef.current;
        if (!size || !currentIdentity) return;
        flushSize(size.cols, size.rows, currentIdentity);
      }, RESIZE_DEBOUNCE_MS);
    }, [flushSize]);

    // 新的运行时有自己独立的 PTY 尺寸，重新允许首个尺寸立即发送，并在就绪后补发
    // 挂载期间量到的尺寸。
    useEffect(() => {
      sentSizeRef.current = null;
      hasSentFirstSizeRef.current = false;
      if (!runtimeId) return;
      const size = pendingSizeRef.current;
      if (size) flushSize(size.cols, size.rows, runtimeId);
    }, [runtimeId, flushSize]);

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
