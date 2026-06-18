import { useState, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import { useSession } from "../../context/SessionContext";
import { pluginRegistry } from "../../core/plugin-registry";
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

interface ConnectDialogProps {
  isOpen: boolean;
  onClose: () => void;
  editSessionId?: string | null;
}

/**
 * 统一新建会话对话框
 *
 * 两步流程：
 *   1. 从 PluginRegistry 动态获取可用协议，选择连接模式
 *   2. 渲染选中插件的配置表单
 *
 * 所有已注册插件均可选——不再有 "Coming Soon" 占位。
 */
export default function ConnectDialog({ isOpen, onClose, editSessionId }: ConnectDialogProps) {
  const { t } = useTranslation();
  const { state, refreshEndpoints, disconnect, switchTab, deleteSession, createOfflineSession } = useSession();

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
  const [transferEnabled, setTransferEnabled] = useState(true);
  const [transferProtocol, setTransferProtocol] = useState<"ymodem" | "xmodem" | "zmodem">("ymodem");
  const [sessionName, setSessionName] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const serialEndpoints = state.endpoints.filter(e => e.connection_type === "serial");
  const isSerial = selectedMode === "serial";

  // 从 PluginRegistry 获取可用协议（替换硬编码列表）
  const availableModes = pluginRegistry.getByCapability("connection").map(p => ({
    id: p.manifest.id,
    icon: p.manifest.icon,
    description: p.manifest.description || p.manifest.name,
    available: true, // 所有已注册插件均可选
    content_type: p.manifest.content_type,
  }));

  // 每次打开对话框时重置
  useEffect(() => {
    if (!isOpen) return;
    refreshEndpoints();
    setError(null);
    setConnecting(false);

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
        if (targetTab.name) setSessionName(targetTab.name);
        if (typeof targetTab.transferEnabled === "boolean") setTransferEnabled(targetTab.transferEnabled);
        if (typeof targetTab.transferProtocol === "string") setTransferProtocol(targetTab.transferProtocol as "ymodem" | "xmodem" | "zmodem");
        return;
      }
    }

    setStep("mode");
    setSelectedMode("serial");
    setPort("");
    setBaudRate("115200");
    setDataBits("8");
    setParity("none");
    setStopBits("1");
    setFlowControl("none");
    setDataMode("text");
    setTransferEnabled(true);
    setTransferProtocol("ymodem");
    setSessionName("");
  }, [isOpen, editSessionId, state.tabs, refreshEndpoints]);

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

  const handleCreate = useCallback(async () => {
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
      transfer_enabled: transferEnabled,
      transfer_protocol: transferProtocol,
    } : {};

    try {
      if (editSessionId) {
        // 编辑模式：先断连，再用新配置替换旧会话
        const targetTab = state.tabs.find(t => t.id === editSessionId);
        if (targetTab?.state === "connected") {
          await disconnect(editSessionId);
        }
      }
      // 仅保存配置，不打开串口（连接由右键菜单触发）
      const sid = await createOfflineSession(
        isSerial ? port : selectedMode, params,
        sessionName || undefined, undefined,
        transferEnabled, transferProtocol,
      );
      if (sid) {
        if (editSessionId) {
          await deleteSession(editSessionId);
        }
        await switchTab(sid);
        onClose();
      }
    } catch (e) {
      setError(String(e));
    }
    setConnecting(false);
  }, [port, isSerial, baudRate, dataBits, parity, stopBits, flowControl, dataMode, transferEnabled, transferProtocol, sessionName, selectedMode, editSessionId, state.tabs, createOfflineSession, disconnect, deleteSession, switchTab, onClose]);

  const handleOverlayClick = useCallback((e: React.MouseEvent) => {
    if (e.target === e.currentTarget) onClose();
  }, [onClose]);

  useEffect(() => {
    if (!isOpen) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, onClose]);

  if (!isOpen) return null;

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
          {/* ── 步骤 1: 模式选择（从 PluginRegistry 动态生成） ── */}
          {step === "mode" && (
            <>
              <h2 className={styles.title}>
                {editSessionId ? (t("contextMenu.reconnect") || "Reconnect") : t("session.newSession")}
              </h2>
              <p className={styles.subtitle}>{t("connectionType.label")}</p>
              <div className={styles.modeGrid}>
                {availableModes.map(mode => (
                  <motion.button
                    key={mode.id}
                    className={styles.modeCard}
                    whileHover={{ scale: 1.03, borderColor: "var(--accent-primary)" }}
                    whileTap={{ scale: 0.97 }}
                    onClick={() => handleModeSelect(mode.id)}
                  >
                    <span className={styles.modeIcon}>{mode.icon}</span>
                    <span className={styles.modeLabel}>{mode.description}</span>
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
                  {availableModes.find(m => m.id === selectedMode)?.icon}{" "}
                  {availableModes.find(m => m.id === selectedMode)?.description}
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

                  {/* 文件传输开关 */}
                  <div className={styles.field}>
                    <label className={styles.checkboxLabel}>
                      <input
                        type="checkbox"
                        checked={transferEnabled}
                        onChange={e => setTransferEnabled(e.target.checked)}
                        disabled={connecting}
                      />
                      <span>{t("serial.enableTransfer")}</span>
                    </label>
                  </div>

                  {/* 传输协议选择（仅启用传输时可见） */}
                  {transferEnabled && (
                    <div className={styles.field}>
                      <label className={styles.label}>{t("serial.transferProtocol")}</label>
                      <select
                        className={styles.select}
                        value={transferProtocol}
                        onChange={e => setTransferProtocol(e.target.value as "ymodem" | "xmodem" | "zmodem")}
                        disabled={connecting}
                      >
                        <option value="ymodem">YModem</option>
                        <option value="xmodem">XModem</option>
                        <option value="zmodem">ZModem</option>
                      </select>
                    </div>
                  )}
                </>
              )}

              {/* ── 未实现插件的占位提示 ── */}
              {!isSerial && (
                <div className={styles.comingSoonBanner} style={{ marginTop: 16 }}>
                  🚧 插件 "{selectedMode}" 的前端配置表单尚未实现，将在后续版本中提供。
                </div>
              )}

              {error && <div className={styles.error}>{error}</div>}

              <div className={styles.actions}>
                <button className={styles.cancelBtn} onClick={handleBack} disabled={connecting}>
                  {t("common.cancel")}
                </button>
                <button
                  className={styles.connectBtn}
                  onClick={handleCreate}
                  disabled={(!port && isSerial) || connecting}
                >
                  {connecting
                    ? t("serial.confirming")
                    : t("serial.confirm")}
                </button>
              </div>
            </>
          )}
        </motion.div>
      </motion.div>
    </AnimatePresence>
  );
}
