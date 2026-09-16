/**
 * iperf 测试记录列表
 *
 * 每次测试（无论谁发起）生成一条记录，点击选中查看详情。
 * 服务端接待的板子测试与客户端发起的测试在同一列表，靠角色标识区分；
 * -d/-r 双向测试的反向相带方向徽标（FWD/REV）。
 */
import { useTranslation } from "react-i18next";
import Icon from "../../components/common/Icon";
import { formatMbps } from "./iperf-utils";
import type { IperfRecord } from "./iperf-events";
import styles from "./IperfSessionView.module.css";

interface Props {
  records: IperfRecord[];
  selectedId: string;
  onSelect: (id: string) => void;
}

export default function IperfRecordList({ records, selectedId, onSelect }: Props) {
  const { t } = useTranslation();
  const units = {
    gbps: t("iperf.unitGbps"),
    mbps: t("iperf.unitMbps"),
    kbps: t("iperf.unitKbps"),
    bps: t("iperf.unitBps"),
  };

  if (records.length === 0) {
    return (
      <div className={`${styles.panel} liquid-glass-card`}>
        <h3>{t("iperf.recordList")}</h3>
        <div className={styles.noData}>{t("iperf.noRecords")}</div>
      </div>
    );
  }

  return (
    <div className={`${styles.panel} liquid-glass-card`}>
      <h3>{t("iperf.recordList")}</h3>
      <div className={styles.recordList}>
        {records.map((record) => {
          const selected = record.id === selectedId;
          const roleLabel = record.role === "server"
            ? t("iperf.server")
            : t("iperf.client");
          const directionLabel = record.direction === "reverse"
            ? "REV"
            : "FWD";
          const endpoint = record.role === "server"
            ? record.peerAddr || "—"
            : record.targetHost || "—";
          const summary = record.summary;
          const bandwidth = summary
            ? formatMbps(summary.bitsPerSecond, units)
            : record.status === "running"
              ? t("iperf.testRunning")
              : "—";

          return (
            <button
              key={record.id}
              type="button"
              className={`${styles.recordItem} ${selected ? styles.recordItemSelected : ""}`}
              onClick={() => onSelect(record.id)}
            >
              <div className={styles.recordTopLine}>
                <span className={styles.recordRole}>
                  {roleLabel}
                  {record.phaseCount > 1 && (
                    <span className={styles.directionBadge}>{directionLabel}</span>
                  )}
                </span>
                <span className={styles.recordStatus}>
                  <Icon
                    name={record.status === "running"
                      ? "transfer-active"
                      : record.status === "done"
                        ? "check-circle"
                        : "x-circle"}
                    size="sm"
                  />
                </span>
              </div>
              <div className={styles.recordEndpoint} title={endpoint}>{endpoint}</div>
              <div className={styles.recordMeta}>
                <span>{bandwidth}</span>
                <span>{new Date(record.startedAt).toLocaleTimeString()}</span>
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}
