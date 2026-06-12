import { useState, useCallback, useRef, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useSerialPort } from "./hooks/useSerialPort";
import { useFileTransfer } from "./hooks/useFileTransfer";
import Terminal from "./components/Terminal/Terminal";
import SerialConfigSidebar from "./components/Sidebar/SerialConfigSidebar";
import FileTransferPanel from "./components/FileTransfer/FileTransferPanel";
import GlassButton from "./components/common/GlassButton";
import Toast from "./components/common/Toast";
import "./i18n/index";
import "./App.css";

/** 侧边栏最小/最大宽度 */
const SIDEBAR_MIN_WIDTH = 200;
const SIDEBAR_MAX_WIDTH = 400;

/** 文件传输面板最小/最大高度 */
const PANEL_MIN_HEIGHT = 24;
const PANEL_MAX_HEIGHT_RATIO = 0.5;

/** Toast 消息类型 */
interface ToastMessage {
  id: number;
  type: "success" | "error" | "warning" | "info";
  message: string;
}

function App() {
  const { t, i18n } = useTranslation();

  // ===== 会话连接 =====
  const {
    status,
    endpoints,
    connectionTypes,
    connectedEndpoint,
    error: serialError,
    refreshEndpoints,
    connect,
    disconnect,
    sendData,
    onData,
    onDisconnect,
    clearError: clearSerialError,
  } = useSerialPort();

  // ===== 文件传输 =====
  const {
    status: transferStatus,
    progress: transferProgress,
    history: transferHistory,
    error: transferError,
    cancelTransfer,
    clearError: clearTransferError,
    clearHistory,
  } = useFileTransfer();

  // ===== 布局状态 =====
  const [sidebarWidth, setSidebarWidth] = useState(280);
  const [isResizingSidebar, setIsResizingSidebar] = useState(false);
  const [panelHeight, setPanelHeight] = useState(PANEL_MIN_HEIGHT);
  const [isResizingPanel, setIsResizingPanel] = useState(false);
  const [isPanelOpen, setIsPanelOpen] = useState(false);

  // ===== Toast =====
  const [toasts, setToasts] = useState<ToastMessage[]>([]);
  const toastIdRef = useRef(0);

  const addToast = useCallback(
    (type: ToastMessage["type"], message: string) => {
      const id = ++toastIdRef.current;
      setToasts((prev) => [...prev, { id, type, message }]);
      setTimeout(() => {
        setToasts((prev) => prev.filter((t) => t.id !== id));
      }, 5000);
    },
    []
  );

  const removeToast = useCallback((id: number) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  // ===== 终端数据回调 =====
  const [terminalWriteData, setTerminalWriteData] = useState<Uint8Array | null>(null);

  useEffect(() => {
    onData((data) => {
      setTerminalWriteData(data);
      // 清除上一次的数据，以便相同数据也能触发更新
      setTimeout(() => setTerminalWriteData(null), 0);
    });
  }, [onData]);

  // ===== 串口状态监控 =====
  useEffect(() => {
    onDisconnect(() => {
      addToast("warning", t("serial.deviceDisconnected"));
    });
  }, [onDisconnect, addToast, t]);

  useEffect(() => {
    if (serialError) {
      addToast("error", serialError);
    }
  }, [serialError, addToast]);

  useEffect(() => {
    if (transferError) {
      addToast("error", transferError);
    }
  }, [transferError, addToast]);

  // ===== 拖拽调整大小（侧边栏） =====
  const sidebarStartX = useRef(0);
  const sidebarStartWidth = useRef(0);

  const handleSidebarMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      setIsResizingSidebar(true);
      sidebarStartX.current = e.clientX;
      sidebarStartWidth.current = sidebarWidth;
    },
    [sidebarWidth]
  );

  // ===== 拖拽调整大小（面板） =====
  const panelStartY = useRef(0);
  const panelStartHeight = useRef(0);

  const handlePanelMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      setIsResizingPanel(true);
      panelStartY.current = e.clientY;
      panelStartHeight.current = panelHeight;
    },
    [panelHeight]
  );

  // ===== 全局鼠标事件 =====
  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      if (isResizingSidebar) {
        const delta = e.clientX - sidebarStartX.current;
        setSidebarWidth(
          Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, sidebarStartWidth.current + delta))
        );
      }
      if (isResizingPanel) {
        const delta = panelStartY.current - e.clientY;
        const maxHeight = window.innerHeight * PANEL_MAX_HEIGHT_RATIO;
        const newHeight = Math.min(maxHeight, Math.max(PANEL_MIN_HEIGHT, panelStartHeight.current + delta));
        setPanelHeight(newHeight);
        if (newHeight > PANEL_MIN_HEIGHT + 10) {
          setIsPanelOpen(true);
        }
      }
    };

    const handleMouseUp = () => {
      setIsResizingSidebar(false);
      setIsResizingPanel(false);
    };

    if (isResizingSidebar || isResizingPanel) {
      document.addEventListener("mousemove", handleMouseMove);
      document.addEventListener("mouseup", handleMouseUp);
      document.body.style.cursor = isResizingSidebar ? "col-resize" : "row-resize";
      document.body.style.userSelect = "none";
    }

    return () => {
      document.removeEventListener("mousemove", handleMouseMove);
      document.removeEventListener("mouseup", handleMouseUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
  }, [isResizingSidebar, isResizingPanel]);

  // ===== 面板切换 =====
  const togglePanel = useCallback(() => {
    if (isPanelOpen) {
      setIsPanelOpen(false);
      setPanelHeight(PANEL_MIN_HEIGHT);
    } else {
      setIsPanelOpen(true);
      setPanelHeight(200);
    }
  }, [isPanelOpen]);

  // ===== 语言切换 =====
  const toggleLanguage = useCallback(() => {
    const newLang = i18n.language === "zh-CN" ? "en-US" : "zh-CN";
    i18n.changeLanguage(newLang);
    localStorage.setItem("tauterm-language", newLang);
    addToast("info", newLang === "zh-CN" ? "已切换为中文" : "Switched to English");
  }, [i18n, addToast]);

  // ===== 键盘快捷键 =====
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Ctrl+Shift+F：切换文件传输面板
      if (e.ctrlKey && e.shiftKey && e.key === "F") {
        e.preventDefault();
        togglePanel();
      }
      // Ctrl+Shift+R：刷新端口列表
      if (e.ctrlKey && e.shiftKey && e.key === "R") {
        e.preventDefault();
        refreshEndpoints();
        addToast("info", t("serial.scanning"));
      }
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [togglePanel, refreshEndpoints, addToast, t]);

  return (
    <div className="app-root">
      <div className="app-layout">
        <div className="app-body">
          {/* 侧边栏 */}
          <aside className="sidebar" style={{ width: sidebarWidth }}>
            <SerialConfigSidebar
              endpoints={endpoints}
              connectionTypes={connectionTypes}
              status={status}
              connectedEndpoint={connectedEndpoint}
              error={serialError}
              onRefresh={refreshEndpoints}
              onConnect={connect}
              onDisconnect={disconnect}
              onClearError={clearSerialError}
            />
          </aside>

          {/* 拖拽手柄 */}
          <div
            className={`sidebar-resize-handle ${isResizingSidebar ? "active" : ""}`}
            onMouseDown={handleSidebarMouseDown}
          />

          {/* 终端视口 */}
          <main className="terminal-viewport">
            <Terminal
              onData={sendData}
              writeData={terminalWriteData}
              isConnected={status === "connected"}
            />
          </main>
        </div>

        {/* 文件传输面板拖拽手柄 */}
        <div
          className={`panel-resize-handle ${isResizingPanel ? "active" : ""}`}
          onMouseDown={handlePanelMouseDown}
          onDoubleClick={togglePanel}
        />

        {/* 文件传输面板 */}
        <div
          className="file-transfer-panel"
          style={{
            height: panelHeight,
            display: isPanelOpen ? "flex" : "none",
          }}
        >
          <FileTransferPanel
            status={transferStatus}
            progress={transferProgress}
            history={transferHistory}
            error={transferError}
            onCancel={cancelTransfer}
            onClearError={clearTransferError}
            onClearHistory={clearHistory}
          />
        </div>

        {/* 状态栏 */}
        <div className="status-bar">
          <div className="status-bar-left">
            <div className="connection-indicator">
              <span
                className={`connection-dot ${
                  status === "connected"
                    ? "connected"
                    : status === "connecting"
                      ? "connecting"
                      : "disconnected"
                }`}
              />
              <span>
                {status === "connected"
                  ? `${t("serial.connected")}: ${connectedEndpoint}`
                  : status === "connecting"
                    ? t("serial.connecting")
                    : t("serial.disconnected")}
              </span>
            </div>
          </div>
          <div className="status-bar-right">
            {/* 语言切换按钮 */}
            <GlassButton
              variant="ghost"
              size="sm"
              onClick={toggleLanguage}
              title={t("settings.language")}
            >
              {i18n.language === "zh-CN" ? "EN" : "中"}
            </GlassButton>
            <span>{t("app.version")} 0.1.0</span>
          </div>
        </div>
      </div>

      {/* Toast 通知 */}
      {toasts.map((toast, index) => (
        <Toast
          key={toast.id}
          type={toast.type}
          message={toast.message}
          index={index}
          onClose={() => removeToast(toast.id)}
        />
      ))}
    </div>
  );
}

export default App;
