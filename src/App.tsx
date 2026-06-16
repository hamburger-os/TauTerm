import { useState, useCallback, useRef, useEffect } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { AnimatePresence, motion } from "framer-motion";
import AppShell from "./components/Layout/AppShell";
import Toolbar from "./components/Layout/Toolbar";
import SessionSidebar from "./components/Layout/SessionSidebar";
import StatusBar from "./components/Layout/StatusBar";
import ResizeHandle from "./components/Layout/ResizeHandle";
import TerminalView from "./components/Terminal/TerminalView";
import BottomPanel from "./components/Layout/BottomPanel";
import CommandPalette from "./components/CommandPalette/CommandPalette";
import ConnectDialog from "./components/Layout/ConnectDialog";
import Toast from "./components/common/Toast";
import { useSession } from "./context/SessionContext";
import { useTransfer } from "./context/TransferContext";
import { useKeyboard } from "./hooks/useKeyboard";
import "./i18n/index";
import "./App.css";

const SIDEBAR_MIN = 180;
const SIDEBAR_MAX = 400;
const PANEL_MIN = 120;
const PANEL_DEFAULT = 250;
const PANEL_MAX_RATIO = 0.5;

interface ToastMessage {
  id: number;
  type: "success" | "error" | "warning" | "info";
  message: string;
}

function AppInner() {
  // Context hooks
  const { state: sessionState, refreshEndpoints } = useSession();
  const { state: transferState, sendFiles: transferSend, receiveFiles: transferReceive, setDragging } = useTransfer();
  const { registerAction } = useKeyboard();

  // Layout state
  const [sidebarWidth, setSidebarWidth] = useState(260);
  const [isResizingSidebar, setIsResizingSidebar] = useState(false);
  const [panelHeight, setPanelHeight] = useState(PANEL_DEFAULT);
  const [isResizingPanel, setIsResizingPanel] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [connectDialogOpen, setConnectDialogOpen] = useState(false);
  const [sidebarVisible, setSidebarVisible] = useState(true);
  const [editSessionId, setEditSessionId] = useState<string | null>(null);

  // Toast
  const [toasts, setToasts] = useState<ToastMessage[]>([]);
  const toastIdRef = useRef(0);
  const addToast = useCallback((type: ToastMessage["type"], message: string) => {
    const id = ++toastIdRef.current;
    setToasts(prev => [...prev, { id, type, message }]);
    setTimeout(() => setToasts(prev => prev.filter(t => t.id !== id)), 5000);
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

  // Resize: bottom panel
  const panelStartY = useRef(0); const panelStartHeight = useRef(0);
  const handlePanelMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault(); setIsResizingPanel(true);
    panelStartY.current = e.clientY; panelStartHeight.current = panelHeight;
  }, [panelHeight]);

  useEffect(() => {
    const handleMove = (e: MouseEvent) => {
      if (isResizingSidebar) {
        setSidebarWidth(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, sidebarStartWidth.current + (e.clientX - sidebarStartX.current))));
      }
      if (isResizingPanel) {
        const maxH = window.innerHeight * PANEL_MAX_RATIO;
        setPanelHeight(Math.min(maxH, Math.max(PANEL_MIN, panelStartHeight.current - (e.clientY - panelStartY.current))));
      }
    };
    const handleUp = () => { setIsResizingSidebar(false); setIsResizingPanel(false); };
    if (isResizingSidebar || isResizingPanel) {
      document.addEventListener("mousemove", handleMove);
      document.addEventListener("mouseup", handleUp);
      document.body.style.cursor = isResizingSidebar ? "col-resize" : "row-resize";
      document.body.style.userSelect = "none";
    }
    return () => {
      document.removeEventListener("mousemove", handleMove);
      document.removeEventListener("mouseup", handleUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
  }, [isResizingSidebar, isResizingPanel]);

  // File transfer handlers
  const handleSendFiles = useCallback(async () => {
    if (!sessionState.activeTabId) return;
    try {
      const selected = await open({ multiple: false, filters: [{ name: "Files", extensions: ["bin", "hex", "elf", "*"] }] });
      if (selected) {
        const paths = Array.isArray(selected) ? selected : [selected];
        transferSend(sessionState.activeTabId, paths);
      }
    } catch (e) { addToast("error", `${e}`); }
  }, [sessionState.activeTabId, transferSend, addToast]);

  const handleReceiveFiles = useCallback(async () => {
    if (!sessionState.activeTabId) return;
    try {
      const selected = await open({ directory: true, multiple: false });
      if (selected && typeof selected === "string") {
        transferReceive(sessionState.activeTabId, selected);
      }
    } catch (e) { addToast("error", `${e}`); }
  }, [sessionState.activeTabId, transferReceive, addToast]);

  // Global drag events for dropzone
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
    return () => {
      document.removeEventListener("dragenter", handleDragEnter);
      document.removeEventListener("dragleave", handleDragLeave);
      document.removeEventListener("dragover", handleDragOver);
      document.removeEventListener("drop", handleDrop);
    };
  }, [setDragging]);

  // Keyboard shortcuts
  useEffect(() => {
    registerAction("palette.open", () => setPaletteOpen(true));
    registerAction("session.new", () => { setEditSessionId(null); setConnectDialogOpen(true); });
    registerAction("sidebar.toggle", () => setSidebarVisible(v => !v));
    registerAction("serial.refresh", refreshEndpoints);
  }, [registerAction, refreshEndpoints]);

  // Command palette execution
  const handlePaletteExecute = useCallback((cmdId: string) => {
    switch (cmdId) {
      case "session.new": setEditSessionId(null); setConnectDialogOpen(true); break;
      case "terminal.search": /* handled by TerminalView */ break;
      case "sidebar.toggle": setSidebarVisible(v => !v); break;
      case "serial.refresh": refreshEndpoints(); break;
      case "palette.open": setPaletteOpen(true); break;
    }
  }, [refreshEndpoints]);

  // Toolbar action handler
  const handleToolbarAction = useCallback((actionId: string) => {
    switch (actionId) {
      case "newSession": setEditSessionId(null); setConnectDialogOpen(true); break;
      case "sidebar": setSidebarVisible(v => !v); break;
      case "commands": setPaletteOpen(true); break;
      case "settings": addToast("info", "设置功能即将推出"); break;
    }
  }, [addToast]);

  return (
    <div className="app-root">
      {/* Toolbar */}
      <Toolbar onAction={handleToolbarAction} />

      <div className="app-body">
        {/* Session Sidebar */}
        <AnimatePresence>
          {sidebarVisible && (
            <motion.aside
              className="sidebar"
              style={{ width: sidebarWidth }}
              initial={{ width: 0, opacity: 0 }}
              animate={{ width: sidebarWidth, opacity: 1 }}
              exit={{ width: 0, opacity: 0 }}
              transition={{ duration: 0.2 }}
            >
              <SessionSidebar
                onEditSession={(id) => { setEditSessionId(id); setConnectDialogOpen(true); }}
              />
            </motion.aside>
          )}
        </AnimatePresence>

        {/* Resize Handle */}
        {sidebarVisible && (
          <ResizeHandle direction="horizontal" onMouseDown={handleSidebarMouseDown} />
        )}

        {/* Main Terminal Area */}
        <main className="terminal-viewport">
          <TerminalView />
        </main>
      </div>

      {/* Panel Resize Handle */}
      <ResizeHandle direction="vertical" onMouseDown={handlePanelMouseDown} />

      {/* Bottom Panel — resizable, tabbed, always visible */}
      <div
        className="file-transfer-panel"
        style={{
          height: panelHeight,
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
        }}
      >
        <BottomPanel
          onSendFiles={handleSendFiles}
          onReceiveFiles={handleReceiveFiles}
        />
      </div>

      {/* Status Bar */}
      <StatusBar />

      {/* Command Palette */}
      <CommandPalette
        isOpen={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        onExecute={handlePaletteExecute}
      />

      {/* Connect Dialog */}
      <ConnectDialog
        isOpen={connectDialogOpen}
        onClose={() => { setConnectDialogOpen(false); setEditSessionId(null); }}
        editSessionId={editSessionId}
      />

      {/* Dropzone Overlay */}
      <AnimatePresence>
        {transferState.isDragging && (
          <motion.div
            className="dropzone-overlay"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
          >
            <motion.div
              className="dropzone-message"
              animate={{ scale: [1, 1.05, 1] }}
              transition={{ repeat: Infinity, duration: 1.5 }}
            >
              ⚡ Drop to Transfer
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Toasts */}
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
