import React, { useState, useCallback, useRef, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { getCurrentWindow } from "@tauri-apps/api/window";               // 窗口状态（最大化/还原追踪）
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AnimatePresence, motion } from "framer-motion";
import AppShell from "./components/Layout/AppShell";
import GoogleGlowBackground from "./components/Layout/GoogleGlowBackground";
import Toolbar from "./components/Layout/Toolbar";
import SessionSidebar from "./components/Layout/SessionSidebar";
import StatusBar from "./components/Layout/StatusBar";
import ResizeHandle from "./components/Layout/ResizeHandle";
import TabContentDispatcher from "./components/TabContentDispatcher";
import SendBar from "./components/SendBar/SendBar";
import type { ProtocolType } from "./types/transfer";
import RightSidebar from "./components/RightSidebar/RightSidebar";
import SessionRightSidebar from "./components/RightSidebar/SessionRightSidebar";
import SettingsPage from "./components/Settings/SettingsPage";
import CommandPalette from "./components/CommandPalette/CommandPalette";
import ConnectDialog from "./components/Layout/ConnectDialog";
import { useToast } from "./context/ToastContext";
import { useSession } from "./context/SessionContext";
import { useTransfer } from "./context/TransferContext";
import { useKeyboard } from "./hooks/useKeyboard";
import { pluginRegistry } from "./core/plugin-registry";
import { ACTION_IDS } from "./shortcuts/actionIds";
import "./i18n/index";
import "./App.css";

const SIDEBAR_MIN = 180;
const SIDEBAR_MAX = 400;
const RIGHT_SIDEBAR_MIN = 160;
const RIGHT_SIDEBAR_MAX_STATIC = 1600; // 静态后备值，实际上限由主内容区宽度动态计算
const RIGHT_SIDEBAR_DEFAULT = 260;
/** SendBar 最小高度（px）：从 CSS 自定义属性 --sendbar-min-height 读取，128 为后备值 */
const SENDBAR_MIN_PCT = 5;
const SENDBAR_MAX_PCT = 80;
const SENDBAR_DEFAULT_PCT = SENDBAR_MIN_PCT;
const RESIZE_DEBOUNCE_MS = 150;

function AppInner() {
  const { t } = useTranslation();
  // Context hooks
  const { state: sessionState, refreshEndpoints, disconnect, switchTab } = useSession();
  const { state: transferState } = useTransfer();
  const { registerAction } = useKeyboard();

  // Layout state
  const [sidebarWidth, setSidebarWidth] = useState(260);
  const [isResizingSidebar, setIsResizingSidebar] = useState(false);
  const [rightSidebarWidth, setRightSidebarWidth] = useState(RIGHT_SIDEBAR_DEFAULT);
  const [isResizingRightSidebar, setIsResizingRightSidebar] = useState(false);
  const [sendBarPct, setSendBarPct] = useState(SENDBAR_DEFAULT_PCT);
  const [isResizingSendBar, setIsResizingSendBar] = useState(false);
  /** SendBar 最小高度，从 CSS 自定义属性 --sendbar-min-height 读取，避免与 SendBar.module.css 硬编码不同步 */
  const [sendbarMinHeight, setSendbarMinHeight] = useState(128);
  const sendbarMinHeightRef = useRef(sendbarMinHeight);
  sendbarMinHeightRef.current = sendbarMinHeight;
  const mainContentRef = useRef<HTMLDivElement>(null);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [connectDialogOpen, setConnectDialogOpen] = useState(false);
  const [sidebarVisible, setSidebarVisible] = useState(true);
  const [rightSidebarVisible, setRightSidebarVisible] = useState(true);
  const [editSessionId, setEditSessionId] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [isMaximized, setIsMaximized] = useState(false);

  // Toast
  const { showToast } = useToast();

  // Event handlers
  useEffect(() => {
    if (sessionState.error) showToast("error", sessionState.error);
  }, [sessionState.error, showToast]);

  useEffect(() => {
    if (transferState.error) showToast("error", transferState.error);
  }, [transferState.error, showToast]);

  // 从 CSS 自定义属性读取 SendBar 最小高度，确保 JS 与 CSS 值一致；
  // 同时修正初始 sendBarPct，避免默认百分比（5%）对应的像素值小于 CSS min-height，
  // 导致首次拖动时 SendBar 出现"跳变高"现象。
  useEffect(() => {
    try {
      const val = getComputedStyle(document.documentElement)
        .getPropertyValue("--sendbar-min-height").trim();
      const parsed = parseInt(val, 10);
      if (!isNaN(parsed)) {
        setSendbarMinHeight(parsed);
        // 修正初始百分比，使其与 CSS min-height 像素值对齐
        const container = mainContentRef.current;
        if (container) {
          const containerHeight = container.clientHeight;
          if (containerHeight > 0) {
            const minPct = Math.max(
              SENDBAR_MIN_PCT,
              Math.ceil((parsed * 100) / containerHeight)
            );
            setSendBarPct(prev => Math.max(prev, minPct));
          }
        }
      }
    } catch { /* 保持默认 128 */ }
  }, []);

  // Resize: sidebar
  const sidebarStartX = useRef(0); const sidebarStartWidth = useRef(0);
  const handleSidebarMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault(); setIsResizingSidebar(true);
    sidebarStartX.current = e.clientX; sidebarStartWidth.current = sidebarWidth;
  }, [sidebarWidth]);

  // Resize: right sidebar
  const rightSidebarStartX = useRef(0); const rightSidebarStartWidth = useRef(0);
  const handleRightSidebarMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault(); setIsResizingRightSidebar(true);
    rightSidebarStartX.current = e.clientX; rightSidebarStartWidth.current = rightSidebarWidth;
  }, [rightSidebarWidth]);

  // Resize: sendBar (flex ratio)
  const sendBarStartY = useRef(0); const sendBarStartPct = useRef(SENDBAR_DEFAULT_PCT);
  const handleSendBarMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault(); setIsResizingSendBar(true);
    sendBarStartY.current = e.clientY; sendBarStartPct.current = sendBarPct;
  }, [sendBarPct]);

  useEffect(() => {
    const resizeActive = isResizingSidebar || isResizingRightSidebar || isResizingSendBar;
    if (!resizeActive) return;

    const handleMove = (e: MouseEvent) => {
      if (isResizingSidebar) {
        setSidebarWidth(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, sidebarStartWidth.current + (e.clientX - sidebarStartX.current))));
      }
      if (isResizingRightSidebar) {
        // 动态上限：主内容区宽度的 80%，保留终端至少 20% 可见空间
        const container = mainContentRef.current;
        const dynamicMax = container
          ? container.getBoundingClientRect().width * 0.8
          : RIGHT_SIDEBAR_MAX_STATIC;
        setRightSidebarWidth(Math.min(dynamicMax, Math.max(RIGHT_SIDEBAR_MIN, rightSidebarStartWidth.current - (e.clientX - rightSidebarStartX.current))));
      }
      if (isResizingSendBar) {
        const container = mainContentRef.current;
        if (!container) return;
        const containerHeight = container.clientHeight;
        if (containerHeight <= 0) return;
        // 向上拖增大 SendBar 占比
        const deltaPct = ((sendBarStartY.current - e.clientY) / containerHeight) * 100;
        // 动态最小百分比：确保 SendBar 不小于 CSS 变量定义的最小高度
        const dynamicMinPct = Math.max(SENDBAR_MIN_PCT, Math.ceil((sendbarMinHeightRef.current * 100) / containerHeight));
        const newPct = Math.min(SENDBAR_MAX_PCT, Math.max(dynamicMinPct, sendBarStartPct.current + deltaPct));
        setSendBarPct(newPct);
      }
    };
    const handleUp = () => {
      setIsResizingSidebar(false);
      setIsResizingRightSidebar(false);
      setIsResizingSendBar(false);
    };
    document.addEventListener("mousemove", handleMove);
    document.addEventListener("mouseup", handleUp);
    document.body.style.cursor = isResizingSendBar ? "row-resize" : "col-resize";
    document.body.style.userSelect = "none";

    return () => {
      document.removeEventListener("mousemove", handleMove);
      document.removeEventListener("mouseup", handleUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
  }, [isResizingSidebar, isResizingRightSidebar, isResizingSendBar]);

  // SSH 主机密钥验证 — 监听后端事件，弹出确认对话框
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    (async () => {
      const fn = await listen<{ fingerprint: string }>(
        "ssh-host-key-verify",
        async (event) => {
          if (cancelled) return;
          const fp = event.payload.fingerprint;
          // 使用原生 OS 确认对话框（不可被 Web 内容伪造，安全性最高）
          const ok = window.confirm(
            `${t("ssh.hostKeyTitle")}\n\n${t("ssh.hostKeyFingerprint")}: ${fp}\n\n${t("ssh.hostKeyPrompt")}`
          );
          try {
            // 显式构造布尔参数，避免 ES6 简写语法在 Tauri IPC 序列化时
            // 可能将 boolean 误序列化为对象的问题
            await invoke("confirm_host_key", {
              fingerprint: fp,
              accepted: ok ? true : false,
            });
          } catch (e) {
            // 后端 oneshot 已被消费（Strict Mode 双重监听器 或 并发连接
            // 使用相同指纹时可能发生），非真实错误，静默忽略。
            const errStr = String(e);
            if (
              errStr.includes("未找到或已过期") ||
              errStr.includes("not found") ||
              errStr.includes("expired")
            ) {
              return;
            }
            showToast("error", t("ssh.hostKeyError", { error: errStr }));
          }
        }
      );
      // await listen 返回时 cleanup 可能已执行 — 此时 cancelled=true，
      // 立即取消刚注册的 listener 防止泄漏
      if (cancelled) {
        fn();
        return;
      }
      unlisten = fn;
    })();
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, [showToast, t]);

  // 窗口最大化/还原状态追踪
  useEffect(() => {
    const appWindow = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    let resizeTimer: ReturnType<typeof setTimeout>;

    // 初始状态
    appWindow.isMaximized().then(setIsMaximized).catch(() => {});

    // 监听窗口大小变化（防抖避免 IPC 风暴）
    appWindow.onResized(() => {
      clearTimeout(resizeTimer);
      resizeTimer = setTimeout(async () => {
        const maximized = await appWindow.isMaximized();
        setIsMaximized(prev => prev === maximized ? prev : maximized);
      }, RESIZE_DEBOUNCE_MS);
    }).then(fn => { unlisten = fn; });

    return () => {
      clearTimeout(resizeTimer);
      unlisten?.();
    };
  }, []);

  // Keyboard shortcuts — stable actions (register once on mount)
  useEffect(() => {
    registerAction(ACTION_IDS.PALETTE_OPEN, () => setPaletteOpen(true));
    registerAction(ACTION_IDS.SESSION_NEW, () => { setEditSessionId(null); setConnectDialogOpen(true); });
    registerAction(ACTION_IDS.SIDEBAR_TOGGLE, () => setSidebarVisible(v => !v));
    registerAction(ACTION_IDS.RIGHT_SIDEBAR_TOGGLE, () => setRightSidebarVisible(v => !v));
    registerAction(ACTION_IDS.SERIAL_REFRESH, refreshEndpoints);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Keyboard shortcuts — session-dependent actions (re-register on tab changes)
  useEffect(() => {
    registerAction(ACTION_IDS.SESSION_CLOSE, () => {
      if (sessionState.activeTabId) disconnect(sessionState.activeTabId);
    });
    registerAction(ACTION_IDS.SESSION_NEXT, () => {
      const tabs = sessionState.tabs;
      if (tabs.length === 0) return;
      const idx = tabs.findIndex(t => t.id === sessionState.activeTabId);
      const next = tabs[(idx + 1) % tabs.length];
      switchTab(next.id);
    });
    registerAction(ACTION_IDS.SESSION_PREV, () => {
      const tabs = sessionState.tabs;
      if (tabs.length === 0) return;
      const idx = tabs.findIndex(t => t.id === sessionState.activeTabId);
      const prev = tabs[(idx - 1 + tabs.length) % tabs.length];
      switchTab(prev.id);
    });
  }, [registerAction, disconnect, switchTab, sessionState.tabs, sessionState.activeTabId]);

  // Command palette execution
  const handlePaletteExecute = useCallback((cmdId: string) => {
    switch (cmdId) {
      case ACTION_IDS.SESSION_NEW: setEditSessionId(null); setConnectDialogOpen(true); break;
      case ACTION_IDS.TERMINAL_SEARCH: break;
      case ACTION_IDS.SIDEBAR_TOGGLE: setSidebarVisible(v => !v); break;
      case ACTION_IDS.RIGHT_SIDEBAR_TOGGLE: setRightSidebarVisible(v => !v); break;
      case ACTION_IDS.SERIAL_REFRESH: refreshEndpoints(); break;
      case ACTION_IDS.PALETTE_OPEN: setPaletteOpen(true); break;
    }
  }, [refreshEndpoints]);

  // Toolbar action handler
  const handleToolbarAction = useCallback((actionId: string) => {
    switch (actionId) {
      case "newSession": setEditSessionId(null); setConnectDialogOpen(true); break;
      case "sidebar": setSidebarVisible(v => !v); break;
      case "rightSidebar": setRightSidebarVisible(v => !v); break;
      case "commands": setPaletteOpen(true); break;
      case "settings": setSettingsOpen(true); break;
    }
  }, []);

  return (
    <div className="app-root">
      {/* 动态光球背景层 (z-index: 0) */}
      <GoogleGlowBackground />

      {/* 顶栏 (z-index: 10) */}
      <Toolbar onAction={handleToolbarAction} isMaximized={isMaximized} />

      <div className="app-body">
        {/* 侧栏 — 全高 */}
        <AnimatePresence>
          {sidebarVisible && (
            <>
              <motion.aside
                className="sidebar liquid-glass"
                style={{ width: sidebarWidth }}
                initial={{ width: 0, opacity: 0 }}
                animate={{ width: sidebarWidth, opacity: 1 }}
                exit={{ width: 0, opacity: 0 }}
                transition={{ duration: isResizingSidebar ? 0 : 0.2 }}
              >
                <SessionSidebar
                  onEditSession={(id) => { setEditSessionId(id); setConnectDialogOpen(true); }}
                  onNewSession={() => { setEditSessionId(null); setConnectDialogOpen(true); }}
                  onSettingsClick={() => setSettingsOpen(true)}
                />
              </motion.aside>
              <ResizeHandle direction="horizontal" onMouseDown={handleSidebarMouseDown} />
            </>
          )}
        </AnimatePresence>

        {/* 主内容区：终端 + 传输面板 + 发送栏 */}
        <div className="main-content" ref={mainContentRef}>
          <div className="terminal-transmission-row" style={{ flex: `${100 - sendBarPct} 1 ${100 - sendBarPct}%` }}>
            <main className="terminal-viewport liquid-glass">
              <TabContentDispatcher />
            </main>
            {/* 右侧栏 (按会话隔离，每 tab 一个独立实例) */}
            <AnimatePresence>
              {rightSidebarVisible && (
                <>
                  <ResizeHandle direction="horizontal" onMouseDown={handleRightSidebarMouseDown} />
                  <motion.div
                    style={{ height: "100%", width: rightSidebarWidth, overflow: "hidden" }}
                    initial={{ width: 0, opacity: 0 }}
                    animate={{ width: rightSidebarWidth, opacity: 1 }}
                    exit={{ width: 0, opacity: 0 }}
                    transition={{ duration: isResizingRightSidebar ? 0 : 0.2 }}
                  >
                    <RightSidebar>
                      {sessionState.tabs.map(tab => {
                        const tabPlugin = pluginRegistry.get(tab.pluginId);
                        const showTransmission = tabPlugin
                          ? (tabPlugin.manifest.transfer_protocols?.length ?? 0) > 0 && tab.transferEnabled !== false
                          : false;
                        const showFileManager = tabPlugin?.manifest.id === "ssh" && tab.fileServiceEnabled === true;
                        const isActive = tab.id === sessionState.activeTabId;
                        return (
                          <div
                            key={tab.id}
                            style={isActive ? undefined : { display: "none" }}
                          >
                            <SessionRightSidebar
                              sessionId={tab.id}
                              isConnected={tab.state === "connected" || tab.state === "transferring"}
                              initialProtocol={tab.transferProtocol as ProtocolType | undefined}
                              showTransmission={showTransmission}
                              showFileManager={showFileManager}
                            />
                          </div>
                        );
                      })}
                    </RightSidebar>
                  </motion.div>
                </>
              )}
            </AnimatePresence>
          </div>
          {sessionState.tabs.map(tab => {
            const isActive = tab.id === sessionState.activeTabId;
            const showSendBar = tab.sendBarEnabled !== false;
            return (
              <React.Fragment key={tab.id}>
                {(showSendBar && isActive) && (
                  <ResizeHandle direction="vertical" onMouseDown={handleSendBarMouseDown} />
                )}
                {showSendBar && (
                  <div style={isActive
                    ? { flex: `${sendBarPct} 1 ${sendBarPct}%`, minHeight: sendbarMinHeight, display: 'flex', flexDirection: 'column' as const }
                    : { display: 'none' as const }
                  }>
                    <SendBar sessionId={tab.id} />
                  </div>
                )}
              </React.Fragment>
            );
          })}
        </div>
      </div>

      {/* 状态栏 */}
      <StatusBar />

      {/* 设置页 (全屏覆盖层) */}
      <SettingsPage
        isOpen={settingsOpen}
        onClose={() => setSettingsOpen(false)}
      />

      {/* 命令面板 */}
      <CommandPalette
        isOpen={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        onExecute={handlePaletteExecute}
      />

      {/* 连接对话框 */}
      <ConnectDialog
        isOpen={connectDialogOpen}
        onClose={() => { setConnectDialogOpen(false); setEditSessionId(null); }}
        editSessionId={editSessionId}
      />

      {/* 拖拽调整大小时的全屏透明遮罩层
          确保 mouseup 事件始终在遮罩层（而非底层可能吞事件的禁用元素）上触发，
          同时强制显示正确的 resize 光标 */}
      {(isResizingSidebar || isResizingRightSidebar || isResizingSendBar) && (
        <div
          style={{
            position: "fixed",
            inset: 0,
            zIndex: 35,
            cursor: isResizingSendBar ? "row-resize" : "col-resize",
          }}
          aria-hidden="true"
        />
      )}
    </div>
  );
}

export default function App() {
  return (
    <AppShell>
      <AppInner />
    </AppShell>
  );
}
