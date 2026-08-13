/**
 * iperf 记录详情：实时日志 + 汇总表 + 带宽-时间图表
 */
import { useTranslation } from "react-i18next";
import IperfLogBox from "./IperfLogBox";
import IperfSummaryTable from "./IperfSummaryTable";
import IperfBandwidthChart from "./IperfBandwidthChart";
import type { IperfRecord } from "./iperf-events";
import styles from "./IperfSessionView.module.css";

interface Props {
  record: IperfRecord | null;
}

export default function IperfRecordDetail({ record }: Props) {
  const { t } = useTranslation();

  if (!record) {
    return (
      <div className={`${styles.panel} liquid-glass-card`}>
        <h3>{t("iperf.recordDetail")}</h3>
        <div className={styles.noData}>{t("iperf.noData")}</div>
      </div>
    );
  }

  const chartData = record.intervals.map((i) => ({
    time: i.endSecs,
    bandwidthMbps: i.bandwidthBps / 1e6,
  }));

  return (
    <div className={`${styles.panel} liquid-glass-card`}>
      <div className={styles.panelHeader}>
        <h3>{t("iperf.recordDetail")}</h3>
        {record.status === "running" && (
          <span className={styles.statusRunning}>{t("iperf.testRunning")}</span>
        )}
        {record.status === "completed" && (
          <span className={styles.statusStopped}>{t("iperf.testComplete")}</span>
        )}
        {record.status === "completed" && record.warning && (
          <span className={styles.statusFailed}>{record.warning}</span>
        )}
        {record.status === "failed" && (
          <span className={styles.statusFailed}>{record.error || t("iperf.testFailed")}</span>
        )}
      </div>

      <IperfLogBox lines={record.logLines} />
      <IperfSummaryTable record={record} />
      <div className={styles.chartSection}>
        <h4>{t("iperf.bandwidthChart")}</h4>
        {chartData.length > 0 ? (
          <IperfBandwidthChart dataPoints={chartData} height={160} />
        ) : (
          <div className={styles.noData}>{t("iperf.noData")}</div>
        )}
      </div>
    </div>
  );
}
