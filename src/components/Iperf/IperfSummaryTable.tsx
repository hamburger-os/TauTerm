/**
 * iperf 结构化汇总表（吞吐/抖动/丢包）
 */
import { useTranslation } from "react-i18next";
import { formatBytes, formatMbps } from "./iperf-utils";
import type { IperfRecord } from "./iperf-events";
import styles from "./IperfSessionView.module.css";

interface Props {
  record: IperfRecord;
}

export default function IperfSummaryTable({ record }: Props) {
  const { t } = useTranslation();
  const s = record.summary;

  const rows: Array<[string, string]> = [];
  if (s) {
    rows.push([
      t("iperf.avgBandwidth"),
      formatMbps(s.avgBandwidthBps, {
        gbps: t("iperf.unitGbps"),
        mbps: t("iperf.unitMbps"),
        kbps: t("iperf.unitKbps"),
        bps: t("iperf.unitBps"),
      }),
    ]);
    rows.push([t("iperf.totalBytes"), formatBytes(s.totalBytes)]);
    rows.push([t("iperf.duration"), `${s.durationSecs.toFixed(1)} ${t("iperf.seconds")}`]);
    if (s.jitterMs != null) rows.push([t("iperf.jitter"), `${s.jitterMs.toFixed(3)} ${t("iperf.ms")}`]);
    if (s.lostPercent != null) {
      rows.push([t("iperf.packetLoss"), `${s.lostPercent.toFixed(2)}%`]);
      rows.push([t("iperf.lostPackets"), `${s.lostPackets ?? 0} / ${s.totalPackets ?? 0}`]);
    }
  } else if (record.status === "running") {
    rows.push([t("iperf.testRunning"), "…"]);
  } else {
    rows.push([t("iperf.noData"), "—"]);
  }

  return (
    <div className={styles.summarySection}>
      <h4>{t("iperf.resultSummary")}</h4>
      <table className={styles.summaryTable}>
        <tbody>
          {rows.map(([k, v]) => (
            <tr key={k}>
              <td className={styles.summaryKey}>{k}</td>
              <td className={styles.summaryValue}>{v}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
