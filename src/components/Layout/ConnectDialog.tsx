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

interface ConnectDialogProps {
  isOpen: boolean;
  onClose: () => void;
}

export default function ConnectDialog({ isOpen, onClose }: ConnectDialogProps) {
  const { t } = useTranslation();
  const { state, refreshEndpoints, connect } = useSession();

  const [connType, setConnType] = useState("serial");
  const [port, setPort] = useState("");
  const [baudRate, setBaudRate] = useState("115200");
  const [dataBits, setDataBits] = useState("8");
  const [parity, setParity] = useState("none");
  const [stopBits, setStopBits] = useState("1");
  const [flowControl, setFlowControl] = useState("none");
  const [sessionName, setSessionName] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const serialEndpoints = state.endpoints.filter(e => e.connection_type === "serial");
  const isSerial = connType === "serial";

  // 自动选择第一个可用端口
  useEffect(() => {
    if (isOpen) {
      refreshEndpoints();
      setError(null);
    }
  }, [isOpen, refreshEndpoints]);

  // 从恢复的标签页回填配置
  useEffect(() => {
    if (!isOpen) return;
    const activeTab = state.tabs.find(t => t.id === state.activeTabId);
    if (activeTab?.state === "disconnected" && activeTab.params) {
      const p = activeTab.params;
      if (activeTab.endpoint) setPort(activeTab.endpoint);
      if (typeof p.baud_rate === "number") setBaudRate(String(p.baud_rate));
      if (typeof p.data_bits === "number") setDataBits(String(p.data_bits));
      if (typeof p.parity === "string") setParity(p.parity);
      if (typeof p.stop_bits === "string") setStopBits(p.stop_bits);
      if (typeof p.flow_control === "string") setFlowControl(p.flow_control);
      return;
    }
    // 无恢复标签页时，自动选第一个端口
    if (serialEndpoints.length > 0 && !port) {
      setPort(serialEndpoints[0].name);
    }
  }, [isOpen, state.tabs, state.activeTabId, serialEndpoints, port]);

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
    } : {};

    try {
      const sid = await connect(isSerial ? port : connType, params, sessionName || undefined);
      if (sid) {
        onClose();
        setPort("");
        setSessionName("");
      }
    } catch (e) {
      setError(String(e));
    }
    setConnecting(false);
  }, [port, isSerial, baudRate, dataBits, parity, stopBits, flowControl, sessionName, connType, connect, onClose]);

  const handleOverlayClick = useCallback((e: React.MouseEvent) => {
    if (e.target === e.currentTarget) onClose();
  }, [onClose]);

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
          <h2 className={styles.title}>{t("session.newSession")}</h2>

          {/* 连接类型 */}
          <div className={styles.field}>
            <label className={styles.label}>{t("connectionType.label")}</label>
            <select className={styles.select} value={connType} onChange={e => setConnType(e.target.value)} disabled={connecting}>
              {state.connectionTypes.map(ct => (
                <option key={ct.id} value={ct.id} disabled={!ct.available}>
                  {t(`connectionType.${ct.id}` as any)}{!ct.available ? ` ${t("connectionType.comingSoon")}` : ""}
                </option>
              ))}
            </select>
          </div>

          {/* 会话名称 */}
          <div className={styles.field}>
            <label className={styles.label}>{t("session.renameSession")} ({t("common.ok")})</label>
            <input
              className={styles.input}
              type="text"
              placeholder={isSerial ? port || "COM3" : "My Session"}
              value={sessionName}
              onChange={e => setSessionName(e.target.value)}
              disabled={connecting}
            />
          </div>

          {/* 串口专用配置 */}
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
            </>
          )}

          {/* 未实现的连接类型 */}
          {!isSerial && (
            <div className={styles.comingSoon}>
              🚧 {t(`connectionType.${connType}` as any)} {t("connectionType.comingSoon")}
            </div>
          )}

          {error && <div className={styles.error}>{error}</div>}

          <div className={styles.actions}>
            <button className={styles.cancelBtn} onClick={onClose} disabled={connecting}>
              {t("common.cancel")}
            </button>
            <button
              className={styles.connectBtn}
              onClick={handleConnect}
              disabled={(!port && isSerial) || connecting || !isSerial}
            >
              {connecting ? t("serial.connecting") : t("serial.connect")}
            </button>
          </div>
        </motion.div>
      </motion.div>
    </AnimatePresence>
  );
}
