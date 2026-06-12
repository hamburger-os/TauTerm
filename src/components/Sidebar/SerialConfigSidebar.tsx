import { useState, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import GlassPanel from "../common/GlassPanel";
import GlassButton from "../common/GlassButton";
import GlassSelect from "../common/GlassSelect";
import GlassInput from "../common/GlassInput";
import type { ConnectionStatus, EndpointInfo, ConnectionTypeInfo } from "../../hooks/useSerialPort";
import styles from "./SerialConfigSidebar.module.css";

/** 波特率选项 */
const BAUD_RATES = [
  { value: "110", label: "110" },
  { value: "300", label: "300" },
  { value: "600", label: "600" },
  { value: "1200", label: "1200" },
  { value: "2400", label: "2400" },
  { value: "4800", label: "4800" },
  { value: "9600", label: "9600" },
  { value: "14400", label: "14400" },
  { value: "19200", label: "19200" },
  { value: "38400", label: "38400" },
  { value: "57600", label: "57600" },
  { value: "115200", label: "115200" },
  { value: "230400", label: "230400" },
  { value: "460800", label: "460800" },
  { value: "921600", label: "921600" },
];

const DATA_BITS_OPTIONS = [
  { value: "5", label: "5" }, { value: "6", label: "6" },
  { value: "7", label: "7" }, { value: "8", label: "8" },
];

const PARITY_OPTIONS = [
  { value: "none", label: "无 (None)" },
  { value: "even", label: "偶校验 (Even)" },
  { value: "odd", label: "奇校验 (Odd)" },
];

const STOP_BITS_OPTIONS = [
  { value: "1", label: "1" }, { value: "2", label: "2" },
];

const FLOW_CONTROL_OPTIONS = [
  { value: "none", label: "无 (None)" },
  { value: "rts_cts", label: "RTS/CTS (硬件)" },
  { value: "xon_xoff", label: "XON/XOFF (软件)" },
];

interface SerialConfigSidebarProps {
  /** 端点列表 */
  endpoints: EndpointInfo[];
  /** 可用连接类型 */
  connectionTypes: ConnectionTypeInfo[];
  /** 连接状态 */
  status: ConnectionStatus;
  /** 已连接的端点 */
  connectedEndpoint: string | null;
  /** 已连接的连接类型（保留用于未来显示） */
  connectedType?: string | null;
  /** 错误信息 */
  error: string | null;
  /** 刷新端点列表 */
  onRefresh: () => void;
  /** 连接到端点 */
  onConnect: (endpoint: string, params: Record<string, unknown>) => void;
  /** 断开连接 */
  onDisconnect: () => void;
  /** 清除错误 */
  onClearError: () => void;
}

/**
 * 终端连接配置侧边栏
 *
 * 支持选择连接类型（串口/SSH/Telnet），显示对应配置项。
 * 当前仅串口已实现，SSH/Telnet 显示为"即将推出"。
 */
export default function SerialConfigSidebar({
  endpoints,
  connectionTypes,
  status,
  connectedEndpoint,
  error,
  onRefresh,
  onConnect,
  onDisconnect,
  onClearError,
}: SerialConfigSidebarProps) {
  const { t } = useTranslation();

  const [selectedType, setSelectedType] = useState("serial");
  const [selectedEndpoint, setSelectedEndpoint] = useState("");
  const [baudRate, setBaudRate] = useState("115200");
  const [dataBits, setDataBits] = useState("8");
  const [parity, setParity] = useState("none");
  const [stopBits, setStopBits] = useState("1");
  const [flowControl, setFlowControl] = useState("none");
  const [customBaudRate, setCustomBaudRate] = useState("");

  const isConnected = status === "connected";
  const isConnecting = status === "connecting";
  const useCustomBaud = baudRate === "custom";

  // 过滤当前类型的端点
  const typeEndpoints = endpoints.filter((ep) => ep.connection_type === selectedType);

  const endpointOptions = typeEndpoints.length > 0
    ? typeEndpoints.map((ep) => ({ value: ep.name, label: ep.name }))
    : [{ value: "", label: t("serial.noPorts") }];

  // 连接类型选项（使用 i18n 翻译）
  const typeLabel = useCallback(
    (id: string) => t(`connectionType.${id}` as unknown as Parameters<typeof t>[0], { defaultValue: id }),
    [t]
  );
  const typeOptions = connectionTypes.map((ct) => ({
    value: ct.id,
    label: ct.available ? typeLabel(ct.id) : `${typeLabel(ct.id)} ${t("connectionType.comingSoon")}`,
  }));

  // 自动选择第一个端点
  useEffect(() => {
    if (!selectedEndpoint && typeEndpoints.length > 0) {
      setSelectedEndpoint(typeEndpoints[0].name);
    }
  }, [typeEndpoints, selectedEndpoint]);

  const handleConnect = useCallback(() => {
    if (!selectedEndpoint) return;
    const finalBaudRate = useCustomBaud ? parseInt(customBaudRate) || 115200 : parseInt(baudRate);
    onConnect(selectedEndpoint, {
      baud_rate: finalBaudRate,
      data_bits: parseInt(dataBits),
      parity,
      stop_bits: stopBits,
      flow_control: flowControl,
    });
  }, [selectedEndpoint, baudRate, dataBits, parity, stopBits, flowControl, customBaudRate, useCustomBaud, onConnect]);

  const handleDisconnect = useCallback(() => {
    onDisconnect();
    onClearError();
  }, [onDisconnect, onClearError]);

  const isSerialType = selectedType === "serial";
  const selectedTypeAvailable = connectionTypes.find(ct => ct.id === selectedType)?.available ?? false;

  return (
    <div className={styles.sidebar}>
      <GlassPanel padding="md">
        <h2 className={styles.title}>{t("serial.title")}</h2>

        <div className={styles.form}>
          {/* 连接类型选择 */}
          <GlassSelect
            label={t("connectionType.label")}
            options={typeOptions}
            value={selectedType}
            onChange={(e) => setSelectedType(e.target.value)}
            disabled={isConnected || isConnecting}
            fullWidth
          />

          {/* 端点选择 */}
          <div className={styles.fieldRow}>
            <GlassSelect
              label={t("serial.port")}
              options={endpointOptions}
              value={selectedEndpoint}
              onChange={(e) => setSelectedEndpoint(e.target.value)}
              disabled={isConnected || isConnecting}
              fullWidth
            />
            <GlassButton
              variant="ghost"
              size="sm"
              onClick={onRefresh}
              disabled={isConnecting}
              title={t("serial.refresh")}
            >
              ↻
            </GlassButton>
          </div>

          {/* 串口专用配置 */}
          {isSerialType && (
            <>
              <GlassSelect
                label={t("serial.baudRate")}
                options={[...BAUD_RATES, { value: "custom", label: "自定义..." }]}
                value={baudRate}
                onChange={(e) => setBaudRate(e.target.value)}
                disabled={isConnected}
                fullWidth
              />
              {useCustomBaud && (
                <GlassInput
                  type="number"
                  placeholder="输入自定义波特率..."
                  value={customBaudRate}
                  onChange={(e) => setCustomBaudRate(e.target.value)}
                  disabled={isConnected}
                  fullWidth
                />
              )}
              <GlassSelect label={t("serial.dataBits")} options={DATA_BITS_OPTIONS} value={dataBits} onChange={(e) => setDataBits(e.target.value)} disabled={isConnected} fullWidth />
              <GlassSelect label={t("serial.parity")} options={PARITY_OPTIONS} value={parity} onChange={(e) => setParity(e.target.value)} disabled={isConnected} fullWidth />
              <GlassSelect label={t("serial.stopBits")} options={STOP_BITS_OPTIONS} value={stopBits} onChange={(e) => setStopBits(e.target.value)} disabled={isConnected} fullWidth />
              <GlassSelect label={t("serial.flowControl")} options={FLOW_CONTROL_OPTIONS} value={flowControl} onChange={(e) => setFlowControl(e.target.value)} disabled={isConnected} fullWidth />
            </>
          )}

          {/* 未实现的连接类型 */}
          {!isSerialType && !selectedTypeAvailable && (
            <div className={styles.comingSoon}>
              <p>🚧 {typeLabel(selectedType)} {t("connectionType.comingSoon")}</p>
            </div>
          )}
        </div>

        {/* 连接状态 */}
        {(connectedEndpoint || isConnecting) && (
          <div className={styles.connectedInfo}>
            <span className={`${styles.statusDot} ${isConnected ? styles.connected : styles.connecting}`} />
            <span className={styles.connectedText}>
              {isConnected
                ? `${t("serial.connected")}: ${connectedEndpoint}`
                : t("serial.connecting")}
            </span>
          </div>
        )}

        {/* 错误信息 */}
        {error && (
          <div className={styles.errorBox}>
            <span>{error}</span>
            <button className={styles.errorClose} onClick={onClearError}>×</button>
          </div>
        )}

        {/* 操作按钮 */}
        {selectedTypeAvailable !== false && (
          <div className={styles.actions}>
            {isConnected ? (
              <GlassButton variant="danger" size="lg" fullWidth onClick={handleDisconnect}>
                {t("serial.disconnect")}
              </GlassButton>
            ) : (
              <GlassButton
                variant="primary"
                size="lg"
                fullWidth
                onClick={handleConnect}
                disabled={!selectedEndpoint || isConnecting}
                loading={isConnecting}
              >
                {isConnecting ? t("serial.connecting") : t("serial.connect")}
              </GlassButton>
            )}
          </div>
        )}
      </GlassPanel>
    </div>
  );
}
