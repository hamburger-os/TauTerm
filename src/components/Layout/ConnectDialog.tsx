import { useState, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import { useSession } from "../../context/SessionContext";
import styles from "./ConnectDialog.module.css";

const BAUD_RATES = ["110","300","600","1200","2400","4800","9600","14400","19200","38400","57600","115200","230400","460800","921600"];
const DATA_BITS = ["5","6","7","8"];
const PARITY = [
  { v: "none", l: "None" },
  { v: "even", l: "Even" },
  { v: "odd", l: "Odd" },
];
const STOP_BITS = ["1","2"];
const FLOW_CONTROL = [
  { v: "none", l: "None" },
  { v: "rts_cts", l: "RTS/CTS" },
  { v: "xon_xoff", l: "XON/XOFF" },
];

interface ModeInfo {
  id: string;
  icon: string;
  description: string;
  available: boolean;
}

interface ConnectDialogProps {
  isOpen: boolean;
  onClose: () => void;
  /** 从侧栏右键菜单"配置"进入时，传入要编辑的会话 ID */
  editSessionId?: string | null;
}

/**
 * 全功能新建会话对话框
 *
 * 两步流程：
 *   1. 选择连接模式（Serial、SSH、Telnet、TFTP）
 *   2. 配置该模式的参数
 *
 * 目前仅串口模式可用，其他模式显示"即将推出"。
 */
export default function ConnectDialog({ isOpen, onClose, editSessionId }: ConnectDialogProps) {
  const { t } = useTranslation();
  const { state, refreshEndpoints, connect, disconnect, switchTab } = useSession();

  const [step, setStep] = useState<"mode" | "config">("mode");
  const [selectedMode, setSelectedMode] = useState("serial");

  // 串口配置
  const [port, setPort] = useState("");
  const [baudRate, setBaudRate] = useState("115200");
  const [dataBits, setDataBits] = useState("8");
  const [parity, setParity] = useState("none");
  const [stopBits, setStopBits] = useState("1");
  const [flowControl, setFlowControl] = useState("none");
  const [dataMode, setDataMode] = useState("text");
  const [sessionName, setSessionName] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const serialEndpoints = state.endpoints.filter(e => e.connection_type === "serial");
  const isSerial = selectedMode === "serial";

  // 每次打开对话框时重置
  useEffect(() => {
    if (!isOpen) return;
    refreshEndpoints();
    setError(null);
    setConnecting(false);

    // 如果是从侧栏右键"连接/配置"进入，直接跳到配置步骤并预填参数
    if (editSessionId) {
      const targetTab = state.tabs.find(t => t.id === editSessionId);
      if (targetTab) {
        setSelectedMode(targetTab.connection_type);
        setStep("config");
        if (targetTab.endpoint) setPort(targetTab.endpoint);
        if (targetTab.params) {
          const p = targetTab.params;
          if (typeof p.baud_rate === "number") setBaudRate(String(p.baud_rate));
          if (typeof p.data_bits === "number") setDataBits(String(p.data_bits));
          if (typeof p.parity === "string") setParity(p.parity);
          if (typeof p.stop_bits === "string") setStopBits(p.stop_bits);
          if (typeof p.flow_control === "string") setFlowControl(p.flow_control);
          if (typeof p.data_mode === "string") setDataMode(p.data_mode);
        }
        if (targetTab.name) {
          setSessionName(targetTab.name);
        }
        return; // 跳过默认重置
      }
    }

    // 新建会话：从模式选择开始
    setStep("mode");
    setSelectedMode("serial");
    setPort("");
    setBaudRate("115200");
    setDataBits("8");
    setParity("none");
    setStopBits("1");
    setFlowControl("none");
    setDataMode("text");
    setSessionName("");
  }, [isOpen, editSessionId, state.tabs, refreshEndpoints]);

  // 新建会话进入配置步骤时，自动选第一个可用端口
  useEffect(() => {
    if (!isOpen || step !== "config" || editSessionId) return;
    if (serialEndpoints.length > 0 && !port) {
      setPort(serialEndpoints[0].name);
    }
  }, [isOpen, step, editSessionId, serialEndpoints, port]);

  const handleModeSelect = useCallback((modeId: string) => {
    setSelectedMode(modeId);
    setStep("config");
    setError(null);
  }, []);

  const handleBack = useCallback(() => {
    setStep("mode");
    setError(null);
  }, []);

  const handleConnect = useCallback(async () => {
    if (!port && isSerial) return;
    setError(null);
    setConnecting(true);

    const params: Record<string, unknown> = isSerial ? {
      baud_rate: parseInt(baudRate),
      data_bits: parseInt(dataBits),
      parity,
      stop_bits: stopBits,
      flow_control: flowControl,
      data_mode: dataMode,
    } : {};

    try {
      // 如果正在编辑一个已连接的会话，先断开再以新参数重连
      if (editSessionId) {
        const targetTab = state.tabs.find(t => t.id === editSessionId);
        if (targetTab?.state === "connected") {
          await disconnect(editSessionId);
        }
      }
      const sid = await connect(isSerial ? port : selectedMode, params, sessionName || undefined);
      if (sid) {
        // 立即切换到新会话，确保终端绑定正确的输入
        await switchTab(sid);
        onClose();
      }
    } catch (e) {
      setError(String(e));
    }
    setConnecting(false);
  }, [port, isSerial, baudRate, dataBits, parity, stopBits, flowControl, dataMode, sessionName, selectedMode, editSessionId, state.tabs, connect, disconnect, switchTab, onClose]);

  const handleOverlayClick = useCallback((e: React.MouseEvent) => {
    if (e.target === e.currentTarget) onClose();
  }, [onClose]);

  // Escape 键关闭对话框
  useEffect(() => {
    if (!isOpen) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  // 预定义的模式列表
  const modes: ModeInfo[] = [
    { id: "serial", icon: "🔌", description: t("connectionType.serial") || "Serial Port", available: true },
    { id: "ssh", icon: "🔒", description: t("connectionType.ssh") || "SSH", available: false },
    { id: "telnet", icon: "🌐", description: t("connectionType.telnet") || "Telnet", available: false },
    { id: "tftp", icon: "📡", description: t("connectionType.tftp") || "TFTP", available: false },
  ];

  return (
    <AnimatePresence>
      <motion.div
        className={styles.overlay}
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        onClick={handleOverlayClick}
      >
        <motion.div
          className={styles.dialog}
          initial={{ opacity: 0, y: 20, scale: 0.95 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={{ opacity: 0, y: 20, scale: 0.95 }}
          transition={{ duration: 0.2 }}
        >
          {/* ── 步骤 1: 模式选择 ── */}
          {step === "mode" && (
            <>
              <h2 className={styles.title}>
                {editSessionId ? (t("contextMenu.reconnect") || "Reconnect") : t("session.newSession")}
              </h2>
              <p className={styles.subtitle}>{t("connectionType.label")}</p>
              <div className={styles.modeGrid}>
                {modes.map(mode => (
                  <motion.button
                    key={mode.id}
                    className={`${styles.modeCard} ${!mode.available ? styles.modeCardDisabled : ""}`}
                    whileHover={mode.available ? { scale: 1.03, borderColor: "var(--accent-primary)" } : {}}
                    whileTap={mode.available ? { scale: 0.97 } : {}}
                    onClick={() => mode.available && handleModeSelect(mode.id)}
                    disabled={!mode.available}
                  >
                    <span className={styles.modeIcon}>{mode.icon}</span>
                    <span className={styles.modeLabel}>{mode.description}</span>
                    {!mode.available && (
                      <span className={styles.comingSoonBadge}>{t("connectionType.comingSoon")}</span>
                    )}
                  </motion.button>
                ))}
              </div>
              <div className={styles.actions}>
                <button className={styles.cancelBtn} onClick={onClose}>
                  {t("common.cancel")}
                </button>
              </div>
            </>
          )}

          {/* ── 步骤 2: 配置 ── */}
          {step === "config" && (
            <>
              <div className={styles.configHeader}>
                <button className={styles.backBtn} onClick={handleBack} disabled={connecting}>
                  ← {t("common.back")}
                </button>
                <h2 className={styles.title}>
                  {modes.find(m => m.id === selectedMode)?.icon}{" "}
                  {modes.find(m => m.id === selectedMode)?.description}
                </h2>
              </div>

              {/* 会话名称 */}
              <div className={styles.field}>
                <label className={styles.label}>{t("session.renameSession")} ({t("session.newSession")})</label>
                <input
                  className={styles.input}
                  type="text"
                  placeholder={isSerial ? port || "COM3" : "My Session"}
                  value={sessionName}
                  onChange={e => setSessionName(e.target.value)}
                  disabled={connecting}
                />
              </div>

              {/* ── 串口配置 ── */}
              {isSerial && (
                <>
                  <div className={styles.field}>
                    <label className={styles.label}>{t("serial.port")}</label>
                    <div className={styles.row}>
                      <select className={styles.select} style={{ flex: 1 }} value={port} onChange={e => setPort(e.target.value)} disabled={connecting}>
                        {serialEndpoints.length === 0 && <option value="">{t("serial.noPorts")}</option>}
                        {serialEndpoints.map(ep => (
                          <option key={ep.name} value={ep.name}>{ep.name}{ep.description !== ep.name ? ` — ${ep.description}` : ""}</option>
                        ))}
                      </select>
                      <button className={styles.iconBtn} onClick={refreshEndpoints} title={t("serial.refresh")} disabled={connecting}>↻</button>
                    </div>
                  </div>

                  <div className={styles.row2}>
                    <div className={styles.field}>
                      <label className={styles.label}>{t("serial.baudRate")}</label>
                      <select className={styles.select} value={baudRate} onChange={e => setBaudRate(e.target.value)} disabled={connecting}>
                        {BAUD_RATES.map(b => <option key={b} value={b}>{b}</option>)}
                      </select>
                    </div>
                    <div className={styles.field}>
                      <label className={styles.label}>{t("serial.dataBits")}</label>
                      <select className={styles.select} value={dataBits} onChange={e => setDataBits(e.target.value)} disabled={connecting}>
                        {DATA_BITS.map(d => <option key={d} value={d}>{d}</option>)}
                      </select>
                    </div>
                  </div>

                  <div className={styles.row2}>
                    <div className={styles.field}>
                      <label className={styles.label}>{t("serial.parity")}</label>
                      <select className={styles.select} value={parity} onChange={e => setParity(e.target.value)} disabled={connecting}>
                        {PARITY.map(p => <option key={p.v} value={p.v}>{p.l}</option>)}
                      </select>
                    </div>
                    <div className={styles.field}>
                      <label className={styles.label}>{t("serial.stopBits")}</label>
                      <select className={styles.select} value={stopBits} onChange={e => setStopBits(e.target.value)} disabled={connecting}>
                        {STOP_BITS.map(s => <option key={s} value={s}>{s}</option>)}
                      </select>
                    </div>
                  </div>

                  <div className={styles.field}>
                    <label className={styles.label}>{t("serial.flowControl")}</label>
                    <select className={styles.select} value={flowControl} onChange={e => setFlowControl(e.target.value)} disabled={connecting}>
                      {FLOW_CONTROL.map(f => <option key={f.v} value={f.v}>{f.l}</option>)}
                    </select>
                  </div>

                  <div className={styles.field}>
                    <label className={styles.label}>{t("serial.dataMode")}</label>
                    <select className={styles.select} value={dataMode} onChange={e => setDataMode(e.target.value)} disabled={connecting}>
                      <option value="text">{t("serial.dataModeText")}</option>
                      <option value="hex">{t("serial.dataModeHex")}</option>
                    </select>
                  </div>
                </>
              )}

              {/* ── 未实现模式占位配置 ── */}
              {selectedMode === "ssh" && <PlaceholderConfig fields={["Host / IP Address", "Port (default: 22)", "Username", "Authentication Method"]} />}
              {selectedMode === "telnet" && <PlaceholderConfig fields={["Host / IP Address", "Port (default: 23)", "Terminal Type"]} />}
              {selectedMode === "tftp" && <PlaceholderConfig fields={["Server Address", "Port (default: 69)", "Transfer Mode (netascii/octet)"]} />}

              {error && <div className={styles.error}>{error}</div>}

              <div className={styles.actions}>
                <button className={styles.cancelBtn} onClick={handleBack} disabled={connecting}>
                  {t("common.cancel")}
                </button>
                <button
                  className={styles.connectBtn}
                  onClick={handleConnect}
                  disabled={(!port && isSerial) || connecting || !modes.find(m => m.id === selectedMode)?.available}
                >
                  {connecting
                    ? t("serial.connecting")
                    : (editSessionId && state.tabs.find(t => t.id === editSessionId)?.state === "connected")
                      ? (t("contextMenu.reconnect") || "Reconnect")
                      : t("serial.connect")}
                </button>
              </div>
            </>
          )}
        </motion.div>
      </motion.div>
    </AnimatePresence>
  );
}

/** 未实现模式的占位配置面板 */
function PlaceholderConfig({ fields }: { fields: string[] }) {
  return (
    <div style={{ padding: "16px 0" }}>
      {fields.map((field, i) => (
        <div key={i} className={styles.field} style={{ marginBottom: 10 }}>
          <label className={styles.label}>{field}</label>
          <input
            className={styles.input}
            type="text"
            placeholder={field}
            disabled
            style={{ opacity: 0.4 }}
          />
        </div>
      ))}
      <div className={styles.comingSoonBanner}>
        🚧 {fields.length > 0 ? "此连接模式即将推出" : "Coming Soon"}
      </div>
    </div>
  );
}
