import React, { useState, useCallback, useRef, useEffect } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow"; // 拖放事件（onDragDropEvent 仅 WebviewWindow 类型可用）
import { getCurrentWindow } from "@tauri-apps/api/window";               // 窗口状态（最大化/还原追踪）
import { AnimatePresence, motion } from "framer-motion";
import AppShell from "./components/Layout/AppShell";
import GoogleGlowBackground from "./components/Layout/GoogleGlowBackground";
import Toolbar from "./components/Layout/Toolbar";
import SessionSidebar from "./components/Layout/SessionSidebar";
import StatusBar from "./components/Layout/StatusBar";
import ResizeHandle from "./components/Layout/ResizeHandle";
import TabContentDispatcher from "./components/TabContentDispatcher";
import SendBar from "./components/SendBar/SendBar";
import TransmissionPanel from "./components/Transmission/TransmissionPanel";
import type { ProtocolType } from "./types/transfer";
import SettingsPage from "./components/Settings/SettingsPage";
import CommandPalette from "./components/CommandPalette/CommandPalette";
import ConnectDialog from "./components/Layout/ConnectDialog";
import Icon from "./components/common/Icon";
import Toast from "./components/common/Toast";
import { useSession } from "./context/SessionContext";
import { useTransfer } from "./context/TransferContext";
import { useKeyboard } from "./hooks/useKeyboard";
import { pluginRegistry } from "./core/plugin-registry";
import { ACTION_IDS } from "./shortcuts/actionIds";
import "./i18n/index";
import "./App.css";

const SIDEBAR_MIN = 180;
const SIDEBAR_MAX = 400;
const TRANSMISSION_MIN = 160;
const TRANSMISSION_MAX = 500;
const TRANSMISSION_DEFAULT = 260;
const RESIZE_DEBOUNCE_MS = 150;

interface ToastMessage {
  id: number;
  type: "success" | "error" | "warning" | "info";
  message: string;
}

function AppInner() {
  // Context hooks
  const { state: sessionState, refreshEndpoints } = useSession();
  const { state: transferState, sendFiles: transferSend, setDragging } = useTransfer();
  const { registerAction } = useKeyboard();

  // Layout state
  const [sidebarWidth, setSidebarWidth] = useState(260);
  const [isResizingSidebar, setIsResizingSidebar] = useState(false);
  const [transmissionWidth, setTransmissionWidth] = useState(TRANSMISSION_DEFAULT);
  const [isResizingTransmission, setIsResizingTransmission] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [connectDialogOpen, setConnectDialogOpen] = useState(false);
  const [sidebarVisible, setSidebarVisible] = useState(true);
  const [editSessionId, setEditSessionId] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [isMaximized, setIsMaximized] = useState(false);

  // Toast
  const [toasts, setToasts] = useState<ToastMessage[]>([]);
  const toastIdRef = useRef(0);
  const addToast = useCallback((type: ToastMessage["type"], message: string) => {
    const id = ++toastIdRef.current;
    setToasts(prev => [...prev, { id, type, message }]);
  }, []);

  // Event handlers
  useEffect(() => {
    if (sessionState.error) addToast("error", sessionState.error);
  }, [sessionState.error, addToast]);

  useEffect(() => {
    if (transferState.error) addToast("error", transferState.error);
  }, [transferState.error, addToast]);

  // Resize: sidebar
  const sidebarStartX = useRef(0); const sidebarStartWidth = useRef(0);
  const handleSidebarMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault(); setIsResizingSidebar(true);
    sidebarStartX.current = e.clientX; sidebarStartWidth.current = sidebarWidth;
  }, [sidebarWidth]);

  // Resize: transmission panel
  const transmissionStartX = useRef(0); const transmissionStartWidth = useRef(0);
  const handleTransmissionMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault(); setIsResizingTransmission(true);
    transmissionStartX.current = e.clientX; transmissionStartWidth.current = transmissionWidth;
  }, [transmissionWidth]);

  useEffect(() => {
    const resizeActive = isResizingSidebar || isResizingTransmission;
    if (!resizeActive) return;

    const handleMove = (e: MouseEvent) => {
      if (isResizingSidebar) {
        setSidebarWidth(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, sidebarStartWidth.current + (e.clientX - sidebarStartX.current))));
      }
      if (isResizingTransmission) {
        // 向左拖动增大面板，向右拖动减小面板
        setTransmissionWidth(Math.min(TRANSMISSION_MAX, Math.max(TRANSMISSION_MIN, transmissionStartWidth.current - (e.clientX - transmissionStartX.current))));
      }
    };
    const handleUp = () => { setIsResizingSidebar(false); setIsResizingTransmission(false); };
    document.addEventListener("mousemove", handleMove);
    document.addEventListener("mouseup", handleUp);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";

    return () => {
      document.removeEventListener("mousemove", handleMove);
      document.removeEventListener("mouseup", handleUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
  }, [isResizingSidebar, isResizingTransmission]);

  // Global drag events for dropzone visual feedback + Tauri native drop
  useEffect(() => {
    const handleDragEnter = (e: DragEvent) => { e.preventDefault(); setDragging(true); };
    const handleDragLeave = (e: DragEvent) => {
      if (e.clientX <= 0 || e.clientY <= 0 || e.clientX >= window.innerWidth || e.clientY >= window.innerHeight) {
        setDragging(false);
      }
    };
    const handleDragOver = (e: DragEvent) => { e.preventDefault(); };
    const handleDrop = (e: DragEvent) => { e.preventDefault(); setDragging(false); };
    document.addEventListener("dragenter", handleDragEnter);
    document.addEventListener("dragleave", handleDragLeave);
    document.addEventListener("dragover", handleDragOver);
    document.addEventListener("drop", handleDrop);

    let unlistenDrop: (() => void) | undefined;
    (async () => {
      try {
        unlistenDrop = await getCurrentWebviewWindow().onDragDropEvent((event) => {
          if (event.payload.type === "drop") {
            setDragging(false);
            const paths = event.payload.paths;
            if (paths && paths.length > 0) {
              if (sessionState.activeTabId) {
                transferSend(sessionState.activeTabId, paths);
              } else {
                addToast("warning", "请先连接到串口设备再拖拽传输文件");
              }
            }
          } else if (event.payload.type === "over") {
            setDragging(true);
          } else if (event.payload.type === "leave") {
            setDragging(false);
          }
        });
      } catch (e) {
        console.warn("Tauri drag-drop 事件注册失败，拖拽传文件功能不可用:", e);
      }
    })();

    return () => {
      document.removeEventListener("dragenter", handleDragEnter);
      document.removeEventListener("dragleave", handleDragLeave);
      document.removeEventListener("dragover", handleDragOver);
      document.removeEventListener("drop", handleDrop);
      if (unlistenDrop) unlistenDrop();
    };
  }, [setDragging, sessionState.activeTabId, transferSend, addToast]);

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

  // Keyboard shortcuts
  useEffect(() => {
    registerAction(ACTION_IDS.PALETTE_OPEN, () => setPaletteOpen(true));
    registerAction(ACTION_IDS.SESSION_NEW, () => { setEditSessionId(null); setConnectDialogOpen(true); });
    registerAction(ACTION_IDS.SIDEBAR_TOGGLE, () => setSidebarVisible(v => !v));
    registerAction(ACTION_IDS.SERIAL_REFRESH, refreshEndpoints);
  }, [registerAction, refreshEndpoints]);

  // Command palette execution
  const handlePaletteExecute = useCallback((cmdId: string) => {
    switch (cmdId) {
      case ACTION_IDS.SESSION_NEW: setEditSessionId(null); setConnectDialogOpen(true); break;
      case ACTION_IDS.TERMINAL_SEARCH: break;
      case ACTION_IDS.SIDEBAR_TOGGLE: setSidebarVisible(v => !v); break;
      case ACTION_IDS.SERIAL_REFRESH: refreshEndpoints(); break;
      case ACTION_IDS.PALETTE_OPEN: setPaletteOpen(true); break;
    }
  }, [refreshEndpoints]);

  // Toolbar action handler
  const handleToolbarAction = useCallback((actionId: string) => {
    switch (actionId) {
      case "newSession": setEditSessionId(null); setConnectDialogOpen(true); break;
      case "sidebar": setSidebarVisible(v => !v); break;
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
            <motion.aside
              className="sidebar liquid-glass"
              style={{ width: sidebarWidth }}
              initial={{ width: 0, opacity: 0 }}
              animate={{ width: sidebarWidth, opacity: 1 }}
              exit={{ width: 0, opacity: 0 }}
              transition={{ duration: 0.2 }}
            >
              <SessionSidebar
                onEditSession={(id) => { setEditSessionId(id); setConnectDialogOpen(true); }}
                onNewSession={() => { setEditSessionId(null); setConnectDialogOpen(true); }}
                onSettingsClick={() => setSettingsOpen(true)}
              />
            </motion.aside>
          )}
        </AnimatePresence>

        {/* 侧栏拖拽条 */}
        {sidebarVisible && (
          <ResizeHandle direction="horizontal" onMouseDown={handleSidebarMouseDown} />
        )}

        {/* 主内容区：终端 + 传输面板 + 发送栏 */}
        <div className="main-content">
          <div className="terminal-transmission-row">
            <main className="terminal-viewport liquid-glass">
              <TabContentDispatcher />
            </main>
            {sessionState.tabs.map(tab => {
                const tabPlugin = pluginRegistry.get(tab.pluginId);
                const tabShowTransmission = tabPlugin
                  ? (tabPlugin.manifest.transfer_protocols?.length ?? 0) > 0 && tab.transferEnabled !== false
                  : false;
                const isActive = tab.id === sessionState.activeTabId;
                return (
                  <React.Fragment key={tab.id}>
                    {isActive && tabShowTransmission && (
                      <ResizeHandle direction="horizontal" onMouseDown={handleTransmissionMouseDown} />
                    )}
                    {isActive && tabShowTransmission && (
                      <div style={{ width: transmissionWidth }}>
                        <TransmissionPanel
                          sessionId={tab.id}
                          isConnected={tab.state === "connected" || tab.state === "transferring"}
                          initialProtocol={tab.transferProtocol as ProtocolType | undefined}
                        />
                      </div>
                    )}
                  </React.Fragment>
                );
              })}
          </div>
          {sessionState.tabs.map(tab => {
            const isActive = tab.id === sessionState.activeTabId;
            return (
              <div key={tab.id} style={{ display: isActive ? undefined : "none" }}>
                <SendBar sessionId={tab.id} isActive={isActive} />
              </div>
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

      {/* 拖拽覆盖层 */}
      <AnimatePresence>
        {transferState.isDragging && (
          <motion.div
            className="dropzone-overlay glass-overlay"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
          >
            <motion.div
              className="dropzone-message"
              animate={{ scale: [1, 1.05, 1] }}
              transition={{ repeat: Infinity, duration: 1.5 }}
            >
              <Icon name="logo" size="lg" /> Drop to Transfer
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Toast 通知 */}
      {toasts.map((toast, index) => (
        <Toast key={toast.id} type={toast.type} message={toast.message} index={index} onClose={() => {
          setToasts(prev => prev.filter(t => t.id !== toast.id));
        }} />
      ))}
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
