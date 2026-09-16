/**
 * iperf 客户端面板
 *
 * 全部测试参数（目标/端口/协议/-t/-b/-P/-i + 版本特有选项）+ 运行/停止 + 命令预览。
 * 瞬态任务：配置 → 运行 → 实时出结果 → 结束。
 */
import { useTranslation } from "react-i18next";
import Icon from "../common/Icon";
import IperfCommandPreview from "./IperfCommandPreview";
import {
  buildIperfCommand,
  formatWindowSize,
  parseBits,
  parseWindowSize,
} from "./iperf-utils";
import type { IperfParams, IperfVersionStr } from "./iperf-events";
import styles from "./IperfSessionView.module.css";

interface Props {
  version: IperfVersionStr;
  params: IperfParams;
  targetHost: string;
  testRunning: boolean;
  error: string | null;
  onParamsChange: (p: IperfParams) => void;
  onTargetHostChange: (host: string) => void;
  onRun: () => void;
  onStop: () => void;
}

export default function IperfClientPanel({
  version,
  params,
  targetHost,
  testRunning,
  error,
  onParamsChange,
  onTargetHostChange,
  onRun,
  onStop,
}: Props) {
  const { t } = useTranslation();

  // 带宽输入框的字符串状态（"100M"），保持用户原始输入
  const bandwidthStr = params.bandwidthBps != null
    ? (params.bandwidthBps >= 1e9
        ? (params.bandwidthBps / 1e9).toFixed(0) + "G"
        : params.bandwidthBps >= 1e6
          ? (params.bandwidthBps / 1e6).toFixed(0) + "M"
          : String(params.bandwidthBps))
    : "";

  // -w 输入框的字符串状态（"64K"），保持用户原始输入
  const windowSizeStr =
    params.windowSize != null ? formatWindowSize(params.windowSize) : "";

  const set = (patch: Partial<IperfParams>) =>
    onParamsChange({ ...params, ...patch });

  // 端口校验：空输入忽略（不产生 0），越界忽略（输入框 min/max 之外的
  // 手动输入不落参数——用户可继续修正）
  const setPort = (raw: string) => {
    if (raw === "") return;
    const n = Number(raw);
    if (!Number.isInteger(n) || n < 1 || n > 65535) return;
    set({ port: n });
  };

  const setBandwidth = (raw: string) => {
    if (raw.trim() === "") {
      set({ bandwidthBps: null });
      return;
    }
    const bps = parseBits(raw);
    if (bps != null) set({ bandwidthBps: bps });
  };

  // -w（1024 进制，对齐 iperf2 byte_atoi）；空 = 系统默认
  const setWindowSize = (raw: string) => {
    if (raw.trim() === "") {
      set({ windowSize: null });
      return;
    }
    const n = parseWindowSize(raw);
    if (n != null) set({ windowSize: n });
  };

  const setNum = (patch: (n: number) => Partial<IperfParams>) => (
    raw: string
  ) => {
    const n = Number(raw);
    if (!isNaN(n) && n >= 0) set(patch(n));
  };

  // -d/-r 互斥（对齐真实 iperf2：两者取后者语义）；-d/-r 当前仅支持 TCP
  const dirDisabled = testRunning || params.protocol === "udp";
  const toggleBidirectional = (checked: boolean) =>
    set({ bidirectional: checked, tradeoff: checked ? false : params.tradeoff });
  const toggleTradeoff = (checked: boolean) =>
    set({ tradeoff: checked, bidirectional: checked ? false : params.bidirectional });

  const command = buildIperfCommand({
    version,
    role: "client",
    targetHost,
    listenPort: params.port,
    protocol: params.protocol,
    durationSecs: params.durationSecs,
    port: params.port,
    parallelStreams: params.parallelStreams,
    reportIntervalSecs: params.reportIntervalSecs,
    bandwidthBps: params.bandwidthBps,
    bidirectional: params.bidirectional,
    tradeoff: params.tradeoff,
    windowSize: params.windowSize,
    reverse: params.reverse,
    bidir: params.bidir,
    omitSecs: params.omitSecs,
  });

  return (
    <div className={`${styles.panel} liquid-glass-card`}>
      <div className={styles.panelHeader}>
        <h3>{t("iperf.client")}</h3>
        {testRunning && (
          <span className={styles.statusRunning}>{t("iperf.testRunning")}</span>
        )}
      </div>

      {/* 目标主机 + 端口 */}
      <div className={styles.row2}>
        <div className={styles.field}>
          <label>{t("iperf.targetHost")}</label>
          <input
            type="text"
            placeholder={t("iperf.targetHostPlaceholder")}
            value={targetHost}
            onChange={(e) => onTargetHostChange(e.target.value)}
            disabled={testRunning}
            className="liquid-glass-input"
          />
        </div>
        <div className={styles.field}>
          <label>{t("iperf.port")}</label>
          <input
            type="number"
            min={1}
            max={65535}
            value={params.port}
            onChange={(e) => setPort(e.target.value)}
            disabled={testRunning}
            className="liquid-glass-input"
          />
        </div>
      </div>

      {/* 协议 + 时长 */}
      <div className={styles.row2}>
        <div className={styles.field}>
          <label>{t("iperf.protocol")}</label>
          <select
            value={params.protocol}
            onChange={(e) => set({ protocol: e.target.value as "tcp" | "udp" })}
            disabled={testRunning}
            className="liquid-glass-input liquid-glass-select"
          >
            <option value="tcp">{t("iperf.protocolTcp")}</option>
            <option value="udp">{t("iperf.protocolUdp")}</option>
          </select>
        </div>
        <div className={styles.field}>
          <label>{t("iperf.duration")}</label>
          <input
            type="number"
            min={1}
            max={3600}
            value={params.durationSecs}
            onChange={(e) => setNum((n) => ({ durationSecs: Math.max(1, Math.min(3600, n)) }))(e.target.value)}
            disabled={testRunning}
            className="liquid-glass-input"
          />
        </div>
      </div>

      {/* 带宽 + 并行流 */}
      <div className={styles.row2}>
        <div className={styles.field}>
          <label>{t("iperf.bandwidth")}</label>
          <input
            type="text"
            placeholder={t("iperf.bandwidthHelp")}
            value={bandwidthStr}
            onChange={(e) => setBandwidth(e.target.value)}
            disabled={testRunning}
            className="liquid-glass-input"
          />
        </div>
        <div className={styles.field}>
          <label>{t("iperf.parallelStreams")}</label>
          <input
            type="number"
            min={1}
            max={64}
            value={params.parallelStreams}
            onChange={(e) => setNum((n) => ({ parallelStreams: Math.max(1, Math.min(64, n)) }))(e.target.value)}
            disabled={testRunning}
            className="liquid-glass-input"
          />
        </div>
      </div>

      {/* 报告间隔 */}
      <div className={styles.field}>
        <label>{t("iperf.reportInterval")}</label>
        <input
          type="number"
          min={1}
          max={60}
          value={params.reportIntervalSecs}
          onChange={(e) => setNum((n) => ({ reportIntervalSecs: Math.max(1, Math.min(60, n)) }))(e.target.value)}
          disabled={testRunning}
          className="liquid-glass-input"
        />
      </div>

      {/* iperf2 特有：-d（双向同时）/ -r（顺序反向）/ -w（socket 缓冲） */}
      {version === "iperf2" && (
        <>
          <div className={styles.checkboxRow}>
            <label
              className={`liquid-glass-toggle ${styles.checkboxLabel}`}
              title={params.protocol === "udp" ? t("iperf.tcpOnly") : undefined}
            >
              <input
                type="checkbox"
                checked={params.bidirectional}
                onChange={(e) => toggleBidirectional(e.target.checked)}
                disabled={dirDisabled}
              />
              <div />
              <span>{t("iperf.bidirectional")}</span>
            </label>
            <label
              className={`liquid-glass-toggle ${styles.checkboxLabel}`}
              title={params.protocol === "udp" ? t("iperf.tcpOnly") : undefined}
            >
              <input
                type="checkbox"
                checked={params.tradeoff}
                onChange={(e) => toggleTradeoff(e.target.checked)}
                disabled={dirDisabled}
              />
              <div />
              <span>{t("iperf.tradeoff")}</span>
            </label>
          </div>
          <div className={styles.field}>
            <label>{t("iperf.windowSize")}</label>
            <input
              type="text"
              placeholder={t("iperf.windowSizeHelp")}
              value={windowSizeStr}
              onChange={(e) => setWindowSize(e.target.value)}
              disabled={testRunning}
              className="liquid-glass-input"
            />
          </div>
        </>
      )}

      {/* iperf3 特有：-R / --bidir / -O */}
      {version === "iperf3" && (
        <>
          <div className={styles.checkboxRow}>
            <label className={`liquid-glass-toggle ${styles.checkboxLabel}`}>
              <input
                type="checkbox"
                checked={params.reverse}
                onChange={(e) => set({ reverse: e.target.checked })}
                disabled={testRunning}
              />
              <div />
              <span>{t("iperf.reverse")}</span>
            </label>
            <label className={`liquid-glass-toggle ${styles.checkboxLabel}`}>
              <input
                type="checkbox"
                checked={params.bidir}
                onChange={(e) => set({ bidir: e.target.checked })}
                disabled={testRunning}
              />
              <div />
              <span>{t("iperf.bidir")}</span>
            </label>
          </div>
          <div className={styles.field}>
            <label>{t("iperf.omit")}</label>
            <input
              type="number"
              min={0}
              max={60}
              value={params.omitSecs}
              onChange={(e) => setNum((n) => ({ omitSecs: Math.max(0, Math.min(60, n)) }))(e.target.value)}
              disabled={testRunning}
              className="liquid-glass-input"
            />
          </div>
        </>
      )}

      {/* 运行 / 停止 */}
      <div className={styles.actions}>
        {testRunning ? (
          <button
            className={`${styles.dangerBtn} liquid-glass-button`}
            onClick={onStop}
          >
            <Icon name="stop" size="sm" /> {t("iperf.stop")}
          </button>
        ) : (
          <button
            className={`${styles.primaryBtn} liquid-primary-button`}
            onClick={onRun}
            disabled={!targetHost}
            title={!targetHost ? t("iperf.targetHostRequired") : ""}
          >
            <Icon name="play" size="sm" /> {t("iperf.run")}
          </button>
        )}
      </div>

      {/* 错误提示（可见，不静默） */}
      {error && <div className={styles.errorText}>{error}</div>}

      {/* 当前角色命令预览（提示性） */}
      <IperfCommandPreview command={command} />
    </div>
  );
}
