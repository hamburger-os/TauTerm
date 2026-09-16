/**
 * iperf 测试记录列表
 *
 * 每次测试（无论谁发起）生成一条记录，点击选中查看详情。
 * 服务端接待的板子测试与客户端发起的测试在同一列表，靠角色标识区分；
 * -d/-r 双向测试的反向相带方向徽标（FWD/REV）。
 */
import { useTranslation } from "react-i18next";
import Icon from "../common/Icon";
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
        {records.map((r) => (
          <button
            key={r.id}
            className={`${styles.recordItem} ${r.id === selectedId ? styles.recordItemActive : ""}`}
            onClick={() => onSelect(r.id)}
          >
            <div className={styles.recordRow1}>
              <span className={`${styles.roleBadge} ${r.role === "server" ? styles.roleServer : styles.roleClient}`}>
                {r.role === "server" ? t("iperf.recordServer") : t("iperf.recordClient")}
              </span>
              {r.direction === "rev" && (
                <span className={styles.directionBadge}>
                  {t("iperf.directionRev")}
                </span>
              )}
              <span className={styles.recordVersion}>{r.version}</span>
              <span className={styles.recordProtocol}>{r.protocol.toUpperCase()}</span>
              <span className={styles.recordStatus}>
                {r.status === "running" && <Icon name="hourglass" size="xs" />}
                {r.status === "completed" && <Icon name="check-circle" size="xs" />}
                {r.status === "failed" && <Icon name="x-circle" size="xs" />}
              </span>
            </div>
            <div className={styles.recordRow2}>
              <span className={styles.recordTime}>
                {new Date(r.startTime).toLocaleTimeString()}
              </span>
              <span className={styles.recordBw}>
                {r.summary
                  ? formatMbps(r.summary.avgBandwidthBps, units)
                  : r.status === "running"
                    ? "…"
                    : r.status === "failed"
                      ? (r.error || t("iperf.testFailed"))
                      : t("iperf.noData")}
              </span>
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}
